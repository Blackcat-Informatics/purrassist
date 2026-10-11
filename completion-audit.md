# Completion audit (Stage 2): issue #15, PR #19, `purrassist-derive`

## Scope and authority

- **Reviewed:** worktree `.worktrees/15-purrassist-derive-zero-dependency`, branch
  `paudley/15-purrassist-derive-zero-dependency`, head `e4850a8` (equals the PR #19 head
  in `raw/pr-19.json`). Base `origin/main` is `fccfbe7`, which is also the merge base, so
  there is no drift. The tree was clean apart from the untracked `.stage/`.
- **Requirements came from:** `issue.md` (the #15 objective, D12 and #4 Spec A verbatim)
  and `plan.md` (the four verbatim user decisions, D-1..D-14, the completeness contract
  and the plan change). Context: AGENTS.md, .goals, docs/IMPLEMENTATION.md and
  docs/RELEASE.md.
- **Prior evidence adjudicated:** `reviews/completeness-review.md` and its Recheck, and
  `validation.md`.
  - The completeness review's main body ran on `d2410ed`, before the fix. `e4850a8`
    changed `crates/derive/src/{lib,model,parse,write}.rs`, so that evidence is
    invalidated for derive behavior.
  - The Recheck covered the uncommitted fix. Its file scope matches
    `git diff d2410ed..e4850a8`.
  - I did not rely on either. Every behavioral criterion below was re-executed here on
    `e4850a8`.
- **What this audit ran:** read-only on the repository. Scratch crates were built under
  `/tmp/audit15` and deleted afterwards. Nothing was installed. The MSRV run used the
  already-present 1.98.0 validation toolchain.
- **Not run, as instructed:** `make check`, `make package`, `make fuzz` and
  `make wasm-test`.
  - Their results are taken from `validation.md`: `make check` and `make package` exit 0
    on `e4850a8`, and `make wasm-test` passed on `534f862`.
  - The wasm reuse is valid. `git diff 534f862..HEAD -- crates/core crates/syntax
    crates/host crates/cli Cargo.toml Cargo.lock` is empty (0 lines).
- **Hosted CI:** did not run.

## Criteria

Exit codes are those of the command. "Direct" is `/tmp/audit15/direct`, which depends by
path on `purrassist-derive` and `purrassist-core` only. `cargo tree -e normal,build`
shows exactly those two. "Facade" is `/tmp/audit15/facade`, which depends by path on
`purrassist` only. `cargo tree --depth 1` shows only `purrassist`.

| # | Criterion | Command / demonstration | Observed | Expected | Exit | Status |
|---|---|---|---|---|---|---|
| 1 | Crate layout: `proc-macro = true`, `[lints] workspace`, no `[dependencies]`, README, 3 byte-identical LICENSEs, SPDX | `crates/derive/Cargo.toml`; `cargo metadata --no-deps`; `sha256sum LICENSE-* crates/*/LICENSE-*`; SPDX header grep | Target kind `proc-macro`; the only dependency is `purrassist-core` (dev). Each of the 3 hashes occurs 9 times. SPDX is present in all derive sources, tests, Cargo.toml, README and `derive_support.rs` | as spec | 0 | VERIFIED |
| 2 | Zero dependencies (no syn/quote); core never depends on derive | `python3 scripts/check-repository.py` | "...core and derive boundaries verified."; core deps `[]` | exit 0 | 0 | VERIFIED |
| 3 | First-party parser: attributes, visibility, named fields, `Vec`/`Option`/plain and full paths, raw idents | Direct `bin/probe`: `pub(crate)`, `pub(in crate::inner)`, `::std::vec::Vec<::std::string::String>`, `std::option::Option<u8>`, a type alias, `#[allow]`/`#[cfg_attr]` on fields, a `#[cfg(any())]` field, `/** */` + `#[doc]`, `r#"..."#` description, Unicode variant `Éclair`, `r#type` | All derive. The definition equals the hand-built one; the cfg'd-out field is absent; the block doc gives `"Block doc\nsecond line\nattr doc"` | equality to hand-built | 0 | VERIFIED |
| 4 | Type map: String, i8..i128, isize, u8..u128, usize, f64, `json::Number` (exact), bool, `Vec<T>`, `Option<T>`, nested struct, `ToolEnum` | Direct `AllTypes` (29 fields, including `Vec<Vec<i64>>`, `Vec<Rect>`, `Option<Rect>`, `Vec<Order>`, `Option<Order>`): `assert_eq!` to hand-built → `ToolSet::compile` → schema fragments → hermes turn with every extreme (`i128::MIN`, `u128::MAX`, `"exact":1.000`) → `from_node` | `AllTypes == hand-built`; compiled; `Resolved`/`Resolved`; `from_node == expected`; `exact=1.000`, `u128=340282366920938463463374607431768211455` | equality and round trip | 0 | VERIFIED |
| 5 | `#[tool(name, alias, description, effects = mutating\|read_only, policy = structural\|tolerant, unknown = error\|drop\|pass)]` | Direct `Finder`, `WriteFile` and `Open` definitions `assert_eq!` to hand-built. Compiled accessors `effects/policy/unknown_parameters/description`. `lookup_exact("find"/"search")`. Real hermes turns for each `unknown` value | Equal. Accessors are `(ReadOnly, Tolerant, Error, "Explicit tool text")` and `(Mutating, Structural, DropWithLoss, "Write the\nfile.")`. Alias `search` → tool `find_things`. `error`: call `Unresolved{UnknownParameter{"/junk"}}`. `drop`: `BestGuess`, args `{"path":"a"}`. `pass`: `Resolved`, extra `{"zz":null,"yy":{"deep":[1,"x"]}}` | all keys and values honored | 0 | VERIFIED |
| 6 | `#[param(name, alias, description, repeat, default = <json>)]`, plus `pattern` and `extra` (plan D-7, user decision) | Direct `ParamKeys` (18 fields) `assert_eq!` to hand-built. `argument_shape().resolve_name("query"/"text")`. `defaults()`. Schema contains the repeat array and the pattern. Defaults of every JSON kind: `10`, `-1.5`, `true`, `false`, an escaped string, `[1,2,3,]`, an object, an object with a trailing comma, an enum string, `1e400`, `[[1],[]]` (probe adds `-0`, `0.5e-3`, `1E5`, `-1e400`). Pattern violation through recovery | Equal; aliases resolve to `q`; `defaults()` lists all 11; `"tags":{"type":"array","items":{"type":"string"}}`. The pattern violation gives call `Unresolved{SchemaValidation}` | as spec | 0 | VERIFIED |
| 7 | User decision **naming** ("snake_case tool, field idents, verbatim variants"; explicit wins, incl. on variants) | Direct: names of `ReadFile`, `HTTPRequest`, `ReadFile2`, `Read2File`, `already_snake`, `#[tool(name="explicit.name")]`; field `r#type`; `Bare`/`Order` values | `["read_file","http_request","read_file2","read2_file","already_snake","explicit.name"]`; `type`; values `X`,`Y` verbatim; `#[param(name="descending")]` wins; `"Desc"` is rejected | as decided | 0 | VERIFIED |
| 8 | User decision **defaults filled in FromNode**; `Option<T>` + default gives `Some(default)` | Direct hermes turn omitting every defaulted member → `from_node` | `limit:10, offset:-1.5, flag:true, other_flag:false, greeting:"s\"qé", nums:[1,2,3], area:Rect{3,Some(4)}, maybe_area:Some(Rect{5,None}), ord:Natural, big:1e400, nested_default:[[1],[]]` | defaults filled | 0 | VERIFIED |
| 9 | User decision **`unknown = pass` with an `extra` field**; `compile_error!` without `extra` and for `extra` without `pass` | Direct and facade: full `recover` → `from_node` in hermes and qwen3-coder-xml. Rejections r16, r17, r19, r33 and f2 | The hermes extra is `{"zz":null,...}`; the qwen extra is `{"color":"blue"}`; with no undeclared members it is `{}`. Rejection texts are in row 15 | as decided | 0 / 101 | VERIFIED |
| 10 | User decision **doc comments** for structs, fields, enums and variants, explicit wins | Direct `Order`, `Mode`, `OnlyVariantDocs`, `AllTypes.mode`, `ParamKeys.q`; facade `SearchIndex`, `Direction` | Struct and field docs are descriptions and explicit keys win. The `order` schema is `"How results are ordered.\n- Asc: Smallest first.\n- descending: Largest first"`. `mode` is `"Explicit enum text\n- Fast: Quick"`, replaced by the field's own `"Own text."`. `only` is `"- A: one\n- B: two"` | as decided (variants composed, see A-2) | 0 | VERIFIED |
| 11 | Full compile → recover (real hermes and qwen3-coder-xml turns) → `from_node` | Direct `ParamKeys` qwen turn (alias tag `text`, repeated `tags`/`labels`, repeated nested `boxes` items, nested `area`, enum, exact `2.50`). Facade `SearchIndex` hermes turn (tool alias `find`, param alias `q`, `Vec<Window>`) and qwen turn (repeated tags, nested windows items, `1e-3`) | Every call is `Resolved`/`Resolved` and each `from_node` equals the expected struct | round trip | 0 | VERIFIED |
| 12 | Exact `FromNodeError`s: missing, duplicate, unknown, mismatch, nested pointer, u8 range, non-object | Direct `Check` with 12 hand-made nodes, plus the u8=300 hermes turn; facade `/windows/0/width` | `/name required member/missing`; `/name a single occurrence/duplicate member`; `/zzz a declared member/undeclared member`; `/name string/number 5`; `/count integer within the u8 range/number 300` (recovered call `Resolved`, then conversion fails); `/items/1/sub`; `/items/0/sub missing`; `/items/0/x undeclared`; `/order one of "Asc", "descending", "Natural"/string`; `"" object/array of 1 items`. A duplicate is never reported as missing | exact errors | 0 | VERIFIED |
| 13 | Wrong-type default caught twice; a `null` default rejected | Direct `BadDefault` (`"x"` for `u32`) and `NullDefault` | compile gives `InvalidDefinition{bad_default, ...does not conform...}`; `from_node({})` gives `/n integer within the u32 range/string`. The null default gives `InvalidDefinition` | as D-5/D-13 | 0 | VERIFIED |
| 14 | Facade (D12): `#[derive(purrassist::ToolDefinition/ToolEnum)]` with no `crate =` and no direct core dependency; no collision with the `tool_call::ToolDefinition` type; hygiene | Facade crate. A `mod glob { use purrassist::*; use purrassist::tool_call::ToolDefinition; #[derive(purrassist::ToolDefinition)] ... }`. A `mod shadowed` with `const node`, `struct members/Option/Result/Vec/String`, `mod core/std/purrassist_core` beside a derive | Compiles and runs: `compiled: ["g","http_request","search_index","shadow"]`; the shadowed struct converts | works with `purrassist` only | 0 | VERIFIED |
| 15 | ≥8 rejection cases fail to compile with the intended error (generics, tuple, union, unknown attribute keys, and more) | Direct `cargo check --bin rNN` for 38 cases, plus 4 facade cases (`f1`–`f4`) | **All 42 exit 101 with the intended error at the intended token.** Examples: generic, lifetime and where clause give "generic parameters are not supported"/"`where` clauses are not supported"; tuple gives "tuple structs are not supported"; union gives "unions cannot describe tool arguments"; unknown `#[tool]`/`#[param]` keys give "unknown key `nmae` ... expected one of"; "duplicate key `name`"; enum↔struct misuse; "variant `B` carries data"; E0277 `String: ToolRepeatable`, `u32: ToolPattern`, `Option<String>: ToolItem` (both `Vec<Option>` and `Option<Option>`), `HashMap: ToolParameter`, `String: ToolExtra`; pass/extra misuse (4 messages); `0x10`, `1_000` and `10u8` give "is not a JSON number"; an ident, byte string or char gives "must be a JSON value"; self-reference (`Vec<S>` and `Option<Box<Self>>`) gives "refers to `S` itself"; indirect recursion gives E0391 at `#[derive]`; `naïve` and an invalid alias give E0080 with the remedy; `Größe` gives "not a valid tool name ... add #[tool(name = ..)]"; duplicate parameter and enum values give "declared twice"; an empty enum; a bad `effects` value; a wrong key on an enum or variant | intended error per case | 101 each | VERIFIED |
| 16 | In-repo evidence: `crates/derive/tests/derive.rs`, compile_fail doctests with twins, `crates/purrassist/tests/derive_facade.rs`, host facade test | `cargo test -p purrassist-derive -p purrassist` (spec check 1); `cargo test -p purrassist-host --test derive_facade --locked` | 27 unit, 24 integration, 45 doctests, 42 compile_fail (9 with error codes enforced by the pinned nightly rustdoc), plus facade 3 integration and 5 doctests; host 1 passed | all pass | 0 / 0 | VERIFIED |
| 17 | Spec check: `cargo test -p purrassist-derive -p purrassist` | run on `e4850a8` | all suites `ok`, 0 failed | exit 0 | 0 | VERIFIED |
| 18 | Spec check: `python3 scripts/check-release.py` | run | "Release set and dependency order verified for 0.1.0: purrassist-core, purrassist-derive, purrassist-syntax, purrassist, purrassist-host, purrassist-cli"; publish.yml verified | exit 0 | 0 | VERIFIED |
| 19 | Spec check: `python3 scripts/check-repository.py` | run | "...core and derive boundaries verified." | exit 0 | 0 | VERIFIED |
| 20 | Spec check: `python3 scripts/test-repository-audit.py` (fixtures for derive closure and the `crates/derive/src` scan) | run | Ran 29, OK | exit 0 | 0 | VERIFIED |
| 21 | Spec check: `python3 scripts/test-package-workspace.py` (`NAMES`) | run | Ran 8, OK | exit 0 | 0 | VERIFIED |
| 22 | Spec check: `python3 scripts/test-verify-published.py` (`CRATE_NAMES`, multi-edge `dependencies_of`, real proc-macro fixture) | run | Ran 10 in 98.7s, OK | exit 0 | 0 | VERIFIED |
| 23 | Registration: workspace `members` and dependency; facade dependency and `__core`/`Facade*` re-exports; `RELEASE_ORDER`; publish.yml loop and step name; `packaged_notices.derive = {}`; `check-packages.py` zero-dependency rule; Windows CI job | `git diff fccfbe7..HEAD` on each file; `verify-published.py` loads `RELEASE_ORDER` from check-release | All present. The order is core → derive (D-10): `check-release.py` already requires "normal and dev dependencies [to] precede users", and derive dev-depends on core, so the spec's "derive first" could not pass. Recorded in plan, RELEASE.md and PR body | as spec, with a forced order | n/a | VERIFIED |
| 24 | Docs: RELEASE.md (six crates, order, external one-time steps), IMPLEMENTATION.md "Workspace boundaries", DEPENDENCIES.md paragraph, "six" in RESOURCES.md and the verify-published docstring | diff review; grep for stale "five"/`#4` | All present. RELEASE.md says the derive name reservation and trusted publisher are **not done** and are required before tagging. No stale five-crate or `#4`-derive text remains (the remaining "five" hits are unrelated) | as spec | 0 | VERIFIED |
| 25 | MSRV 1.98 (`rust-version` claim) | `rust-validate 1.98.0 cargo test -p purrassist-derive -p purrassist --locked` (toolchain already present) | same counts as row 16, all ok | exit 0 | 0 | VERIFIED |
| 26 | Crate README example | Its rust block compiled and run as `bin/readme` in Direct | "README OK" | runs | 0 | VERIFIED (not an in-repo doctest; see A-3) |
| 27 | External: crates.io name reservation and trusted publisher for `purrassist-derive` | docs/RELEASE.md | Documented as not done and required before tagging | maintainer action | n/a | UNVERIFIED-EXTERNAL (not an implementation gap) |
| 28 | Full local gate (`make check`, `make package`, `make wasm-test`) | not run (instruction); `validation.md` | `make check` and `make package` exit 0 on `e4850a8`; wasm-test on `534f862`, and core, syntax, host and cli are unchanged since | exit 0 | n/a | Reused, not re-executed |

## Failure-shape checks

- **DARK:** none. The derives are reached from `#[derive(purrassist::...)]` in a crate with
  no other dependency (row 14) and from the core flavor (rows 4–13). Real recovered calls
  convert through the generated `FromNode` (rows 9, 11, 12).
- **REFUSED:** none. The rejected constructs are the spec's own rejection list, plus
  safety rejections the plan decided:
  - recursion, because the builder has no `$ref`;
  - data-carrying variants, because there is no `anyOf` in the builder;
  - invalid names, because core's `ToolSet::compile` would reject them anyway.

  No requested type-map entry or attribute key is rejected.
- **PRODUCER-WITHOUT-CONSUMER:** none.
  - Generated definitions are consumed by `ToolSet::compile` and recovery.
  - `packaged_notices.derive` is consumed by `check-packages.py`.
  - `RELEASE_ORDER` is consumed by publish.yml validation and `verify-published.py`.
- **DUPLICATE:** none. The hand-built builder API remains by design: the derive is
  additive, and the issue does not ask to replace it.
- **LAUNDERED-TO-FOLLOWUP:** none. There is no new `.deficiencies` entry in the root or
  the worktree. Root `.deficiencies` D2 (2026-10-01, a user decision) was the older
  deferral of this very crate, and this PR discharges it. The `#4` retargets to #12–#18
  concern other work that predates this session (the issues exist).
- **TEST-ONLY:** none. Every integration claim was re-executed from downstream crates on
  the real entry points.
- **WEAKENED TESTS:** none. `git diff fccfbe7..HEAD` modifies three pre-existing test
  files and no core test, fixture or golden:
  - `test-package-workspace.py`: the token sentinel was renamed. The loop still asserts
    that all three registry token variables are absent from every child environment.
  - `test-verify-published.py`: extended from 5 to 6 crates, with a real proc-macro crate
    expanded by the facade fixture. No assertion was loosened.
  - `test-repository-audit.py`: additions only.

  In `e4850a8`, `derive.rs` replaced "variant docs have no schema sink" with stronger
  composed-description equalities.
- **VACUOUS OUTPUT:** none. Rendered schemas, `defaults()` and converted structs carry
  the declared content (rows 4–11).
- **SILENT NARROWING:** none found.
  - `json::Number` maps to the `number` schema with an exact-spelling conversion. The
    builder has no separate "exact" kind, and `1.000`, `2.50` and `1e400` are preserved.
  - Integer ranges are enforced by conversion only. This is D-8, documented, and the
    schema builder has no range keywords.
  - "Unknown attributes" are enforced as unknown keys inside `#[tool]`/`#[param]`.
    Foreign attributes must be ignored for coexistence with other derives, and
    unregistered attribute names are already rustc errors.

## Findings

No open REQUIRED findings.

Advisory (non-blocking):

- **A-1 (diagnostic noise).** `#[param(extra)] rest: String` under `unknown = pass`
  produces the intended `E0277 String: ToolExtra` at the field type, plus an extra
  `E0308 mismatched types: expected String, found Node` at `#[derive]`. The generated
  struct literal assigns a `Node`. The crate docs' claim ("rustc trait errors at the
  field's type") holds, but the second error is confusing.
  - Possible fix: suppress it, for example by converting through
    `<T as ToolExtra>`-bounded code.
- **A-2 (variant-doc format).** The user decision says doc lines are "trimmed, joined
  with `\n`". A variant's doc comment is instead whitespace-collapsed to one line inside
  the composed `- value: text` list. Enumeration values have no description sink of
  their own, so this is a necessary formatting choice, and it is documented in
  `crates/derive/src/lib.rs`. The PR body describes the composition but not the collapse.
  - Possible fix: mention the collapse in the PR body.
- **A-3 (untested README).** The `crates/derive/README.md` example is not a doctest. It
  passes when run here, but it can drift. The facade README is doctested.
- **A-4 (plain compile_fail).** 33 of the 42 compile_fail doctests carry no error code,
  because `compile_error!` has none, so they rely on their passing twins. This audit
  independently observed the intended message for every rejection class (row 15).

## Limits

- `make check`, `make package`, `make fuzz` and `make wasm-test` were not re-run (by
  instruction). Their results come from `validation.md` (row 28). Hosted CI did not run.
- The crates.io name reservation and trusted publisher are external and unverified
  (row 27). RELEASE.md and the PR body state that both are not done.
- Standing constraint "cost charged to the governor": the derived conversion implements
  the pre-existing `FromNode` trait from PR #5, which takes no governor. The issue
  requires generating impls of that trait. The conversion is single-pass, O(n log k),
  over an already budget-bounded recovered node, and the core `Members` counter test
  enforces this. Adding governor charging to `FromNode` would be a trait change outside
  #15.

VERDICT: PASS
