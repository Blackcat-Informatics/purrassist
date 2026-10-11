# Completion audit, Stage 2 EXIT: issue #15, PR #19, `purrassist-derive`

## Scope and authority

- **Reviewed:**
  - worktree `.worktrees/15-purrassist-derive-zero-dependency`;
  - branch `paudley/15-purrassist-derive-zero-dependency`, head `d0a99e9` (equals `origin/<branch>` and the PR #19 head);
  - base `origin/main` `fccfbe7`, which is also the merge base, so there is no drift.

  The tree was clean apart from the untracked `.stage/`. There are 12 commits, from `0fb0454` to `d0a99e9`, and none carries an attribution trailer.
- **Read:**
  - context: AGENTS.md, .goals, docs/IMPLEMENTATION.md;
  - `issue.md`, with the body and all six comments;
  - `plan.md`: the four verbatim user decisions, D-1..D-14 and the contract;
  - `gap-analysis.md` (G1-G9), `completion-audit.md` (the entry audit; A-1..A-4) and `reviews/structure-review.md` (F-1..F-6);
  - `remediation-plan.md` (R1-R6 and Declined), whose user decision is "Drop with recorded loss (Recommended)";
  - `reviews/remediation-plan-review.md` (pass 3 PASS) and `tasks/R1..R6-review.md`;
  - `validation.md`;
  - the live PR #19 body and comments (`gh pr view 19`);
  - root `.deficiencies`.
- **Execution surface:**
  - Downstream crates under `/tmp/audit15x`, built with path dependencies on the worktree crates:
    - `direct` (core + derive only; `cargo tree -e normal,build` shows exactly those two);
    - `renamed` / `renamed-neg` (`pa = { package = "purrassist-core" }` + derive);
    - `facade` and `facade-renamed` (only `purrassist`, under that name or as `p`; `cargo tree --depth 1` shows only `purrassist`);
    - `skew` (a copy of core at 0.2.0 with the marker renamed).
  - The R5 preflight ran against a fake registry on 127.0.0.1, behind a URL guard that refuses any non-loopback URL. Nothing contacted crates.io.
  - In-repo commands were read-only test and script runs. No tracked file was modified; nothing was committed or pushed.
- **Reused evidence:** `make check`, `make package`, workspace MSRV and `make wasm-test` on `d0a99e9`, from validation.md. The reuse qualifies:
  - the logs `/tmp/pa15-{check,package,msrv,wasm}.log` end in `check=0`, `package=0` (6 archives, retained under `.cache/package-archives/d0a99e9916ac-*`), `msrv=0` (110 `test result: ok`, 0 FAILED) and `wasm=0`;
  - all four logs are timestamped after the commit time of `d0a99e9` (01:16:52 -0600);
  - the tree is unchanged since.
- **Hosted CI did not run.** All 8 Verify jobs on run 38120841826 did not run.
- **Not performed (external, maintainer only):**
  - the crates.io name reservation and trusted publisher for `purrassist-derive`;
  - the `.deficiencies` D2 resolution record (G8, scheduled for Stage 3).

## Required demonstrations

| # | Requirement | Command / demonstration | Observed | Exit | Verdict |
|---|---|---|---|---|---|
| 1a | **G1 default.** Nested derived struct with no `unknown` key, inside a tool, gets an undeclared member: the call converts and the loss is recorded | `direct/src/bin/g1.rs`: `Point{x,y}` (no key) nested in `Plot{origin: Point, path: Vec<Point>, label}` (mutating default) and `PlotPass{.., #[param(extra)] rest}` at `unknown = pass`. `ToolSet::compile` -> `recover` on a real `hermes.v1` turn and on a `qwen3-coder-xml.v1` turn -> `from_node`. Run: `cargo run -q --bin g1` | All three runs give turn `BestGuess` and call `BestGuess`. Losses `kind=Data rule=tool_call.parameter.drop-unknown dest=/turn/0..144/arguments/origin/color original="color":"red"` and `.../path/0/weight original="weight":9`. `from_node=Ok(PlotPass { origin: Point{x:1,y:2}, path:[Point{x:3,y:4}], rest: {"note":"n"} })`. Qwen gives the same arguments and losses (plus 2 markup framing losses). | 0 | DEMONSTRATED |
| 1b | **G1 `unknown = error`** makes the call unusable | same binary, `err::Point` with `#[tool(unknown = error)]` under a `pass` tool | turn `Resolved`, call `Unresolved { reason: UnknownParameter { pointer: "/origin/color" } }`, `usable=false usable_accepting_guesses=false`; the asserts hold | 0 | DEMONSTRATED |
| 1c | **G1 `unknown = pass` + `extra`** collects the member | same binary, `pass::Point{x,y,#[param(extra)] more}` | turn and call `Resolved`, `usable=true`; `origin.more == {"color":"red"}`, `path[0].more == {"weight":9}` (asserted) | 0 | DEMONSTRATED |
| 1d | G1 edge shapes (beyond the tests) | `direct/src/bin/g1-extra.rs`: `Option<Inner>`, `Box<Inner>`, `Vec<Vec<Inner>>` under `pass`; top-level `unknown = error` with a default nested struct; qwen nested `<parameter=..>` elements | Drops at `/opt/color`, `/boxed/color` and `/grid/0/0/color`, and `from_node` is Ok. Under a strict top level, the nested `/inner/color` is dropped (`BestGuess`) while a top-level `/top` is `UnknownParameter`; this is the documented semantic change. Qwen nested elements give a drop at `/items/0/zz` and `from_node` is Ok. A key that is a near miss of a declared name (`z` vs `a`) is `UnknownParameter`, not dropped; see LOW-1 | 0 | DEMONSTRATED |
| 1e | Native and WASM output are identical (standing constraint) for derived code | `cargo build --release --target wasm32-wasip1 --bin g1 --bin hygiene`; run under Node `node:wasi` (preview1) and natively; `cmp` | `g1: native == wasi (27 lines)`; `hygiene: native == wasi (4 lines)`; both exit 0 on both targets | 0 | DEMONSTRATED |
| 2a | **F-1 hygiene.** Compiles and behaves correctly when user code shadows names | `direct/src/bin/hygiene.rs`, `cargo run -q --bin hygiene`. In the deriving module, it shadows: `mod core/std/alloc/purrassist_core`; the structs `Option/String/Vec/Box/Result/u32/u8/i64/bool/usize/str/f64/Node/Value/Member/Default/Clone/Into/From/FromNode/ToolParameter/Members/Slot`; the consts `Some/None/Ok/Err/node/members`; and the macros `assert!/format!/vec!/panic!/write!/concat!/stringify!`. The derives use defaults, `repeat`, `pattern`, `alias`, nested, `Vec<nested>`, `Option<nested>`, enum, `Box<f64>`, `f32`, `extra` and `pass` | Compiles. Rendered schema is correct. A hermes turn using aliases `sh`/`t` gives `Resolved`, and the converted struct equals the expected one: defaults 7/true/[1,2], `rest={"zz":1}`, `mode=Careful`, `boxed=2.5`, `ratio=0.1`. A hand-made bad node gives `/part/w integer within the u32 range/string`. `HYGIENE OK` | 0 | DEMONSTRATED |
| 2b | F-1 facade-flavor hygiene | `facade/src/bin/facade-hygiene.rs` with `mod purrassist {} mod core {} mod std {}`, shadowed primitives and `assert!` | `t n=1 d=3 m=[Fast] x={"q":true}` | 0 | DEMONSTRATED |
| 2c | F-1 format braces and a shadowed `assert!` cannot disable the name check | `f1-braces.rs`: `#[tool(name = "bad {name} {} {{x}}")]`, `#[param(name = "p {x}")]` and a user `macro_rules! assert` | `error[E0080]: evaluation panicked: tool name "bad {name} {} {{x}}" is not valid: ... or set it with #[tool(name = "..")]`, plus the same for `parameter name "p {x}"`. Twin `f1-braces-twin` (valid names, same shadowed `assert!`) prints `good_name` | 101 / 0 | DEMONSTRATED |
| 2d | F-1/F-6: a non-ASCII derived name gives a spanned error | `f1-nonascii-field.rs` (`größe: u32`), `f1-nonascii-tool.rs` (`struct Größe`) | `error: the parameter name `größe` derived from field `größe` is not a valid parameter name ...; add #[param(name = "..")]` at `4:5` (`^^^^^`, the field ident). `error: the tool name `größe` derived from `Größe` ...; add #[tool(name = "..")]` at `2:8` (the struct ident). Twin `f1-nonascii-twin` (explicit ASCII names on the same idents) prints `measure 1 2`. Variant values stay verbatim (`Éclair` compiles: `f1-nonascii-variant`) | 101 / 0 | DEMONSTRATED |
| 3a | **R4 renamed dependency.** `pa = { package = "purrassist-core", path }` + `#[tool(crate = ::pa)]` compiles and converts | `renamed/` (`cargo tree` shows core + derive only), `cargo run -q`. Enum, nested struct, `unknown = pass` + `extra`, default; hermes turn -> `from_node` | Schema rendered. `from_node: Ok(Run { path: "x", speed: Some(Slow), inner: Inner { a: 4 }, tries: 3, rest: {"q":1} })`. `RENAMED OK` | 0 | DEMONSTRATED |
| 3b | R4 negative control | `renamed-neg/` (same rename, no `crate =`) | `9 error[E0433]: cannot find `purrassist_core` in the list of imported crates`, so the attribute is what makes the rename work | 101 | DEMONSTRATED |
| 3c | **R4 facade flavor** from a crate that depends only on `purrassist` | `facade/` (`cargo tree --depth 1`: only `purrassist`); also `facade-renamed/` (`p = { package = "purrassist" }`, `#[tool(crate = ::p)]` on the enum, `#[tool(crate = ::p::__core)]` on the struct) | Facade: see row 4. Renamed facade: `search Ok(Search { query: "x", order: Some(Asc) })` | 0 | DEMONSTRATED |
| 3d | **R4 simulated version skew** yields an error naming `__DERIVE_ABI_0_1` | `skew/`: core copied with `version = "0.2.0"` and `__DERIVE_ABI_0_1` renamed to `__DERIVE_ABI_0_2` (in `derive_support.rs` and `mod.rs`), real derive 0.1.0; `cargo build` | Exactly one error: `error[E0425]: cannot find value `__DERIVE_ABI_0_1` in module `::purrassist_core::tool_call``, at the `#[derive]`, with `note: similarly named constant `__DERIVE_ABI_0_2` defined here` | 101 | DEMONSTRATED |
| 4 | **Headline example** (root README / PR body `Search` + `Order`): definition JSON, then recovery of a model call, then the typed struct | `facade/src/bin/headline.rs`, `cargo run -q --offline --bin headline` | The README asserts hold verbatim. `json_schema` is `{"type":"object","properties":{"query":{"type":"string","description":"What to look for."},"limit":{"type":"integer","default":10},"order":{"type":"string","enum":["Asc","Desc"]}},"required":["query"],"additionalProperties":false}`, and the OpenAI and Hermes declarations carry `"description":"Search the index."`. A clean hermes turn gives `Resolved`, then `Search { query: "cats", limit: 10, order: Some(Desc) }`. An imperfect turn (single quotes, trailing comma, `'desc'`) gives `Resolved` with evidence `json.syntax.string`×9, `json.syntax.trailing-comma` and `tool_call.value.enum-normalize`, then `Search { query: "black cats", limit: 5, order: Some(Desc) }`. `HEADLINE OK`. A string `'limit': '5'` turn is `Unresolved{SchemaValidation}`; it is not converted, as the docs direct | 0 | DEMONSTRATED |
| 5 | **R5 preflight** pass and fail, on loopback only | `/tmp/audit15x/r5/drive_preflight.py`: it loads the real `scripts/check-release.py`, rebinds only `API` to `http://127.0.0.1:<port>/api/v1/crates`, guards `urlopen` to loopback, and calls the real `main()` with `--preflight-owners --user-agent ..`. This runs `run_preflight`, then `preflight_owners`, then `registry_fetch`/urllib. `fake_registry.py` serves three scenarios | `pass`: `Publish preflight passed: 6 crates exist and share owners blackcat-release, maintainer-a` (12 requests, all carrying the user agent; exit 0). `missing`: `purrassist-derive: ... returned HTTP 404, expected 200 (the crate name must be reserved before tagging)` (exit 1). `owners`: `purrassist-derive: owners ['squatter'] differ from purrassist-core owners [...]` (exit 1). A dead port reports connection refused for every crate (exit 1). `publish.yml` step order: Verify release set (83), then Preflight (90), then Authenticate (92), then the publish loop (95/101). `check-release.py` exit 0; `test-check-release.py` OK | 0 / 1 | DEMONSTRATED |

## Issue #15 acceptance criteria at `d0a99e9`

| Criterion (issue body) | Evidence (this audit unless noted) | Verdict |
|---|---|---|
| Crate layout: `proc-macro = true`, `[lints] workspace`, no `[dependencies]`, README, 3 byte-identical LICENSEs, SPDX | `crates/derive/Cargo.toml` (only `[dev-dependencies] purrassist-core`). `sha256sum LICENSE-* crates/derive/LICENSE-*`: 3 hashes × 2. Every file added by the branch, except the LICENSE files, has an SPDX header. `check-repository.py` exit 0 | DEMONSTRATED |
| Zero dependencies; core never depends on derive; core stays pure | `cargo tree` of `direct`: derive is a leaf proc-macro. `check-repository.py`: "core and derive boundaries verified". A grep of `derive_support.rs` and `crates/derive/src` for env/fs/net/process/time/thread/rand finds nothing | DEMONSTRATED |
| First-party parser (attributes, visibility, named fields, `Vec`/`Option`/paths) | rows 2a and 3a (full paths `::std::vec::Vec<..>`, `::core::option::Option<..>`, `Box`); entry audit row 3 (visibility forms, `macro_rules!` input) still applies: parse.rs changed only by adding the `crate` key (R4) | DEMONSTRATED |
| Generics, tuple structs, unions and unknown attributes give `compile_error!` with a span | standalone `rej-generic` (`2:9` at `<T>`), `rej-tuple` (`2:9`), `rej-union` (`2:1`), `rej-unknown-key` (`nmae` at `2:8`), `rej-unknown-param-key` (`descripton` at `2:20`); all exit 101 with the intended message | DEMONSTRATED |
| Writer via `FromStr`; builder calls plus `FromNode` | row 2a; derived definitions equal hand-built ones (`derive.rs`, passing) | DEMONSTRATED |
| Two macro pairs; facade `__core` and `Facade*` re-exports; no `crate =` needed | rows 3c and 4; `crates/purrassist/src/lib.rs` re-exports | DEMONSTRATED |
| Type map (String, integers, f64, `json::Number` exact, bool, `Vec`, `Option`, nested, enum) | rows 2a and 4; `derive.rs::every_mapped_type_*` passes (entry audit row 4 extremes still covered by these tests) | DEMONSTRATED |
| `#[tool(name, alias, description, effects, policy, unknown = error/drop/pass)]` and `#[param(name, alias, description, repeat, default)]` | rows 1a-1c, 2a (alias `sh`/`t`, repeat, pattern, defaults) and 4 | DEMONSTRATED |
| Registration: members, workspace dependency, facade, `RELEASE_ORDER` (core then derive; D-10 forced by the dev edge), publish.yml, `NAMES`, `CRATE_NAMES`/`dependency_of`, `packaged_notices.derive` | `check-release.py` exit 0 (order core, derive, syntax, purrassist, host, cli); `test-package-workspace.py` OK; `test-verify-published.py` OK | DEMONSTRATED |
| Docs (RELEASE six crates and external steps, IMPLEMENTATION, DEPENDENCIES, RESOURCES, verify-published docstring) | RELEASE.md:31-49 (external steps not done; the preflight fails before core); IMPLEMENTATION.md:57,91-95 | DEMONSTRATED |
| Audit (a) empty closure, (c) scan of `crates/derive/src`, fixtures | `test-repository-audit.py` OK (34) | DEMONSTRATED |
| Tests: `derive.rs`, compile_fail doctests, `derive_facade.rs` | `cargo test -q -p purrassist-derive -p purrassist --locked`: 8 suites ok, 0 failed. `cargo test -p purrassist-host --test derive_facade`: 1 passed. Core `derive_support`: 14 passed | DEMONSTRATED |
| The six issue checks | `cargo test -p purrassist-derive -p purrassist`, `check-release`, `check-repository`, `test-repository-audit`, `test-package-workspace` and `test-verify-published` all exit 0 at `d0a99e9` (`/tmp/audit15x/spec-checks.log`) | DEMONSTRATED |
| User decisions: naming, defaults, `unknown = pass` + `extra`, doc comments; Stage 2 nested default "Drop with recorded loss" | rows 1a-1d, 2a, 2d and 4 (`search`, field names, doc text as description, default 10 filled, extra collected) | DEMONSTRATED |
| Full gate: `make check`, `make package`, MSRV 1.98, `make wasm-test` | reused (see Scope): all exit 0 on `d0a99e9` | DEMONSTRATED (reused) |

## Remediation items at `d0a99e9`

| Item | Evidence | Verdict |
|---|---|---|
| G1 nested policy (R2, user decision) | rows 1a-1d; in-repo cases 1-4 and `Outer2` pass | DEMONSTRATED |
| G2 hosted CI | did not run | EXTERNAL LIMIT |
| G3 publish preflight (R5) | row 5 | DEMONSTRATED |
| G4 explicit enum description + variant lines | `doc_comments_are_descriptions_and_explicit_keys_win` passes (R6 review: falsified by arm deletion) | DEMONSTRATED (test) |
| G5 same-version, crate name, usable-only docs (R4) | derive README:62-80, lib.rs "Using the derives", INTEGRATION.md:1620-1652; rows 3a-3d confirm the stated behavior | DEMONSTRATED |
| G6 README doctest and root drift check | `cargo test -p purrassist-derive --doc -- --list` lists `README.md - ReadmeDoctests (line 16)`; `check-repository.py` exit 0 (drift check); R6 falsification recorded | DEMONSTRATED |
| G7 error codes / harness | codes added where rustc has one; the full harness is Declined on condition of disclosure in the PR body. **The disclosure is absent** (MEDIUM-1) | OPEN (text) |
| G8 `.deficiencies` D2 record | Stage 3 by plan; no new `.deficiencies` entry for #15 | DEFERRED BY PLAN (process) |
| G9 doc widths | R6 review | DEMONSTRATED (review) |
| F-1 name check hygiene | rows 2c and 2d | DEMONSTRATED |
| F-2 primitives, F-3 placeholder idents | row 2a (shadowed `bool/usize/str/u8/u32/f64`); in-repo `placeholder_named_fields_and_variants_convert` passes | DEMONSTRATED |
| F-4 compile-time sorted table; `Members` docs | `__sorted` assertion present; derive tests pass | DEMONSTRATED (tests) |
| F-5 sealed `ToolExtra`, `#[non_exhaustive] Slot`, f32, Box, `ToolParameter` defaults, crate path, version marker | rows 2a (`Box<f64>`, `f32`), 3a-3d and A-1 below; core doctests (sealed impl `compile_fail,E0277`) pass | DEMONSTRATED |
| F-6b number grammar cross-check | `model.rs` unit test against core's parser (derive unit suite: 41 passed) | DEMONSTRATED (test) |
| A-1 single E0277 for a wrong `extra` type | `a1-extra-type.rs` (`rest: String`): exactly one `error[E0277]: the trait bound String: ToolExtra is not satisfied` at `3:41`; "due to 1 previous error" | DEMONSTRATED |
| A-2 the PR body states the variant-description whitespace collapse (R6 plan text) | stated in the R6 PR *comment* and in lib.rs; **not in the PR body** (MEDIUM-1) | OPEN (text) |

## Failure-shape checks

- **DARK:** none. Every new mechanism is reached from a downstream crate through the shipped derive or facade:
  - the nested policy, `crate =`, the marker, `f32`/`Box` and sealed `extra`;
  - the preflight, from `main()`.
- **REFUSED / PLANNED-BY-REFUSAL:** none. The rejections are the spec's own plus recursion and invalid names. No requested type or key is refused.
- **PRODUCER-WITHOUT-CONSUMER / VACUOUS:** none. Schemas, losses and converted structs carry content (rows 1-4).
- **DUPLICATE:** none.
- **LAUNDERED-TO-FOLLOWUP:** none.
  - Root `.deficiencies` has no #15 entry. D2 (2026-10-01, a verbatim user decision) is the older deferral this PR discharges.
  - Declined items are enhancements beyond the issue (`=` pins, F-6a setter, G7 harness). Each has a reason. The G7 decline's disclosure condition is unmet (MEDIUM-1).
- **TEST-ONLY:** none. Every integration claim was re-executed from /tmp crates on real entry points.
- **WEAKENED TESTS:** none found. R2 churn touched only `dims()` and one nested helper, adding the explicit policy (R2 review). No golden was loosened.
- **SILENT NARROWING:** none in code. The near-miss caveat in LOW-1 concerns docs only; the behavior is core's documented rule.

## Findings

### MEDIUM-1: the PR #19 description is stale and partly false at `d0a99e9`, and two remediation commitments that live in it are unmet

**Where:** the PR #19 body (`gh pr view 19`, head `d0a99e9`).

**Problems:**
- **False claim:** "Without them, publishing would stop after core." R5 changed this. The preflight now fails *before* `purrassist-core` publishes: row 5, `publish.yml:90-92`, and docs/RELEASE.md:43-49.
- **Stale validation:** it cites "Final head `e4850a8`", "27 unit, 24 integration and 45 doctests, plus 42 compile-fail doctests" and "`test-repository-audit` (29)". The qualifying runs are on `d0a99e9` (validation.md "Stage 2 full qualification"; there are now 58 compile_fail blocks and 34 audit tests).
- **Changes and decisions omitted:** the Changes table and the "Behaviour (user decisions)" list stop at `e4850a8`. They omit R1-R6, including:
  - the fifth user decision, "Drop with recorded loss": a user-visible recovery-semantics change, under which a nested extra member that used to make a call unresolved now yields a `BestGuess` with a data loss;
  - the new public surface: `#[tool(crate = path)]`, the `__DERIVE_ABI_*` same-major.minor rule, `f32`/`Box<T>`, sealed `ToolExtra`, `#[non_exhaustive] Slot`, `ToolParameter` defaults;
  - the release preflight and `scripts/test-check-release.py` in `make check`.
- **Remediation-plan R6 items not delivered:**
  - "Audit A-2: the PR body states the whitespace collapse of variant descriptions" (it appears only in a PR comment);
  - "this residual [bridge span selection and `compile_error!` emission are covered only by the passing twins] is disclosed in the PR body under residual risks". The Declined G7 harness rests on that disclosure, and it appears nowhere on the PR.

**Consequence:** the PR description is the reviewer-facing claim and normally the squash message. As written, it misstates release behavior and hides a recovery-semantics change and new public API. AGENTS.md requires public documentation to distinguish implemented behavior from plans.

**Required fix:** rewrite the PR body for `d0a99e9`:
- add the R1-R6 commits to Changes;
- add the nested-default user decision verbatim to Behaviour;
- list the new public items;
- point Validation at the `d0a99e9` qualification;
- replace the "stop after core" sentence with the preflight behavior;
- add residual risks for the G7 twin-only coverage of bridge span selection and `compile_error!` emission, and for the variant-description whitespace collapse;
- optionally add the R4 manual evidence (renamed dependency, simulated skew), now also executed in this audit.

**Acceptance:** a focused recheck of the PR body text against this list.

### LOW-1: the nested-default docs overstate "drops"

**Where:**
- `crates/derive/src/lib.rs:196-204`: "A nested object therefore drops an undeclared member and records the loss ... so the call is a `BestGuess` and the derived conversion ... succeeds";
- `crates/derive/README.md:46-49`.

**Behavior:** under the default `policy = structural`, an undeclared member that is a near miss of a declared name is `UnknownParameter`, and the call is unresolved, not dropped. Observed: `{"opt":{"a":1,"z":1}}` gives `Unresolved { UnknownParameter { "/opt/z" } }`. This is core's documented rule (`docs/tool-calls/README.md:225`, `resolve.rs:48-51`), and it is the safer outcome. The derive text states the drop unconditionally.

**Fix:** add "except a near miss of a declared name under `policy = structural`, which leaves the call unresolved, as at the top level".

**Severity:** non-blocking.

### LOW-2: "patch versions may differ" is a release promise with no recorded discipline

**Where:** `crates/derive/README.md:64-65`, `lib.rs` "Same version", INTEGRATION.md:1622.

**The gap:** the marker encodes only major.minor. A future derive 0.1.y that references a core item first added in 0.1.y, paired with an older 0.1.x core, fails with unexplained missing-item errors, not the marker error. docs/RELEASE.md does not record the rule that keeps the promise true. That rule would be either "a derive patch release may not reference core items newer than its minor's first release" or "bump the marker when it does".

**Fix:** add that rule to RELEASE.md.

**Severity:** non-blocking.

### LOW-3: the QUALIFICATION.md evidence row for derive predates Stage 2

**Where:** `docs/QUALIFICATION.md:34`.

**The gap:** it lists `derive.rs`, the compile_fail doctests and both `derive_facade.rs` files, but not:
- `crates/derive/tests/crate_override.rs`;
- the nested-policy cases;
- `f32`/`Box`;
- the ABI-marker tests;
- `scripts/test-check-release.py`.

INTEGRATION.md:1648 and RELEASE.md:64 do cite them.

**Fix:** extend the row.

**Severity:** non-blocking.

## Limits

- Hosted CI did not run, on `main` as well.
- The crates.io name reservation and trusted publisher for `purrassist-derive` are not done. They are external, and RELEASE.md and the preflight treat them as a pre-tag gate.
- `FromNode` takes no governor. This is the PR #5 trait design, and the issue requires impls of that trait. The generated conversion is single-pass, O(n log k), over an already budget-bounded node, and the `Members` counter test guards it. Resource exhaustion in compile and recovery stays explicit through `ToolSet::compile`/`recover` budgets.
- The compile_fail doctests that carry no error code are guarded by their passing twins. This audit observed the intended messages for the spec's four rejection classes, F-1 and A-1.

First-pass verdict (superseded): BLOCKED

## Recheck (head `6cbb628`)

This pass rechecks only the findings above, against head `6cbb628`. That head equals `origin/<branch>` and the PR #19 head, and the tree is clean apart from `.stage/`. The final `VERDICT:` line at the end of this file supersedes the first-pass verdict above.

**Reuse.** `git diff d0a99e9 6cbb628` touches only prose:
- the derive README, outside its code block;
- `//!` paragraphs in `crates/derive/src/lib.rs`;
- RELEASE.md, QUALIFICATION.md and INTEGRATION.md.

No code, manifest or doctest code block changed. The qualification on `d0a99e9` and the demonstrations above therefore still apply. The text-affected checks were rerun on `6cbb628` and pass:
- `cargo test -p purrassist-derive --doc --locked`: 105 doctests (55 runnable, 50 `compile_fail`), 0 failed;
- `RUSTDOCFLAGS="-D warnings" cargo doc -p purrassist-derive --no-deps`: exit 0;
- `check-repository.py`: exit 0;
- `test-check-release.py`: 18 OK;
- `test-repository-audit.py`: 34 OK;
- `make hygiene`: exit 0.

| Finding | Recheck evidence | Verdict |
|---|---|---|
| MEDIUM-1, stale PR body | Read with `gh pr view 19 --repo Blackcat-Informatics/purrassist --json body`; headRefOid is `6cbb628`. Each item checked against the code and against this audit's runs, below. | **DISCHARGED** |
| LOW-1, near-miss wording | `lib.rs` is correct. README and INTEGRATION.md now overstate in the other direction, and execution falsifies them (see the details below) | **STILL-OPEN, re-rated MEDIUM (blocking)** |
| LOW-2, patch rule | RELEASE.md:21-29 is correct | **DISCHARGED** |
| LOW-3, QUALIFICATION row | QUALIFICATION.md:34 is extended | **DISCHARGED** |

**MEDIUM-1 details.**
- **Publish and preflight:** "Without the reservation the new publish preflight stops the job before anything is published". This matches demonstration 5 and `publish.yml:90-95`, and the false "stop after core" sentence is gone.
- **Changes table:** rows R1 `1e17df7` through R6 `d0a99e9`, plus `6cbb628`, are present, and each matches its commit and task review. One example is R3's single E0277 for a wrong `extra` type, observed in A-1 above.
- **User decision:** "Drop with recorded loss" appears verbatim, both in Behaviour and in the R2 row.
- **A-2:** "whitespace inside a variant description collapses to single spaces" is accurate. `/tmp/audit15x/direct/src/bin/a2-collapse.rs` turns a two-line variant doc and `"Tab\there  and\nnewline."` into `- A: First line second line.` and `- B: Tab here and newline.`.
- **G7:** the residual-risk bullet "Compile-error spans" states that bridge span selection and `compile_error!` emission are covered only by twins, and that the harness was declined.
- **Validation:** it cites the `d0a99e9` full qualification with the docs-only rationale for `6cbb628`, which is confirmed above, and states "Hosted CI did not run."
- **Residual risks:** the release prerequisites, version skew, `f32` and integer ranges are all accurate.

Remaining nits do not block:
- "50 derive doctests" should read 105 doctests, 50 of them `compile_fail`.
- The "Exit completion audit" bullet says the findings were "fixed in this body and `6cbb628`". It needs adjusting once the near-miss text below is corrected.

**LOW-1 details.**

Correct text:
- `crates/derive/src/lib.rs:209-214` scopes the exception to structural matching ("under the default structural matching ... such as `z` beside `a` ... is never dropped ... When it is not renamed, the call is unresolved with an unknown parameter"). The earlier demonstration confirms it: `{"opt":{"a":1,"z":1}}` on a mutating tool gives `Unresolved { UnknownParameter "/opt/z" }`. This rerun confirms it again with `nearmiss.rs`: a mutating tool (default Structural) reports `/inner/z` and `/inner/c` as `UnknownParameter` in every case.
- Nit: Structural is the default only for `effects = mutating`, which is the derive's default. `Effects::ReadOnly.default_policy()` is `Tolerant` (`definition.rs:19,44-47`).

Wrong text:
- `crates/derive/README.md:53-56`: "an undeclared key that is a near miss of a declared name (one that name matching would rename) is never dropped, so when it is not renamed the call is unresolved rather than a `BestGuess`".
- `docs/tool-calls/INTEGRATION.md:1590-1592`: "As in core, a near miss of a declared name is never dropped: when name matching does not rename it, the call is `Unresolved` with `UnknownParameter`".

Both are unscoped. Core scopes the rule to Structural: `resolve.rs:48-53` says "except that under `Structural` a near miss ... is never dropped", and `docs/tool-calls/README.md:225` says Tolerant renames by `json.key.match`. So "As in core" misstates core.

Executed: `/tmp/audit15x/direct/src/bin/nearmiss.rs` (`Inner { a: u8, #[param(default = 0)] b: u8 }`, nested, real hermes `recover`). Under Tolerant, both for a `read_only` tool and for `#[tool(policy = tolerant)]`:
- `{"inner":{"a":1,"z":1}}`: `z` is renamed to `b` (`json.key.match`, `BestGuess`).
- `{"inner":{"z":1}}`: `z` is renamed to `a` (`BestGuess`).
- `{"inner":{"a":1,"b":2,"z":1}}` (no free target): the call is `BestGuess` with `tool_call.parameter.drop-unknown` at `/inner/z`. The near miss **is dropped**. It is not unresolved.

**Why the rating rises to MEDIUM.** The new text overstates a safety guarantee in the published crate README and in the integration contract. A caller of a read-only tool who follows the documented `usable_accepting_guesses()` path would run a call whose near-miss value was dropped, while the docs say such a call is unresolved. The loss is recorded, so recovery itself stays honest.

**Required fix.** Scope both sentences to `policy = structural`, which is the default for mutating tools. Add that under `tolerant` the key is renamed by `json.key.match`, which is a guess; when no declared name is free, it is dropped with a recorded loss (`BestGuess`). Then align the PR body's 6cbb628 row and its exit-audit bullet.

**Acceptance.** The README and INTEGRATION.md text agrees with the three Tolerant observations above.

**LOW-2 details.**
- RELEASE.md:21-29 records the rule: a patch release must not change what generated code needs from core, and any support-surface change that an older same-minor core lacks requires a minor bump, which renames the marker.
- The rule is correct for the lockstep model, since both crates take the single workspace version.
- The enforcement claim is true. `the_derive_abi_marker_is_named_after_the_crate_version` (`derive_support.rs:1028-1041`) compares `stringify!(__DERIVE_ABI_0_1)` with `concat!("__DERIVE_ABI_", env!("CARGO_PKG_VERSION_MAJOR"), "_", env!("CARGO_PKG_VERSION_MINOR"))`, so moving the workspace major or minor fails the test until the marker is renamed. Demonstration 3d shows the resulting error.
- Advisory: the rule's wording addresses additions. A core patch that removes or alters a hidden support item which same-minor derive output still names would also break the promise. The first sentence covers this only if read broadly, so one clause would make it explicit.

**LOW-3 details.** QUALIFICATION.md:34 now lists:
- `crate_override.rs`;
- the README doctest;
- the `model`/`write`/`parse` unit tests, including the number agreement test;
- the drift check;
- the nested policy, `f32`/`Box`, `crate =` and `__DERIVE_ABI_*` behaviors.

`scripts/test-check-release.py` is cited in RELEASE.md:74 together with the other release-script evidence. QUALIFICATION.md carries no release-script rows, which is the existing convention.

**Hosted CI** did not run.

Recheck 1 verdict (superseded): BLOCKED

## Recheck 2 (head 4d8a48e)

This pass covers only the items changed by `4d8a48e`:
- the near-miss text in the derive README, in the `lib.rs` crate docs and in INTEGRATION.md;
- the RELEASE.md patch rule;
- the PR #19 body.

`4d8a48e` equals `origin/<branch>` and the PR head. The tree is clean apart from `.stage/`.

**Reuse.** `git diff 6cbb628 4d8a48e` touches prose only, in four files:
- `crates/derive/README.md`, outside its code block;
- `//!` prose in `crates/derive/src/lib.rs`;
- `docs/RELEASE.md`;
- `docs/tool-calls/INTEGRATION.md`.

The earlier demonstrations and the `d0a99e9` qualification therefore still apply. The checks the text can affect were rerun on `4d8a48e`:
- `cargo test -p purrassist-derive --doc --locked`: 55 + 50 passed, 0 failed;
- `RUSTDOCFLAGS="-D warnings" cargo doc -p purrassist-derive --no-deps`: exit 0;
- `check-repository.py`: exit 0;
- `make hygiene`: exit 0.

| Item | Recheck evidence | Verdict |
|---|---|---|
| LOW-1 (re-rated MEDIUM), near-miss wording | The text now matches `nearmiss.rs` and `nearmiss2.rs` (details below) | **DISCHARGED** |
| LOW-2 advisory, removal and change clause | RELEASE.md:28-30 adds the clause | **DISCHARGED** |
| PR #19 body | All requested items match the head (details below) | **ACCURATE** |

**Near-miss wording.**

What the three docs now say:
- README:53-58, `lib.rs:209-217` and INTEGRATION.md:1590-1595 scope the rule by policy.
- Structural, the default for mutating tools: never dropped, and the call is unresolved with `UnknownParameter`.
- Tolerant, the default for read-only tools: renamed to a free declared name as a guess, or dropped with a recorded loss when no declared name is free.
- "As in core" is gone; INTEGRATION.md now says "follow core's matching policy".

Checked against the demonstrations:
- **Default policies:** `effects` sets the default policy (`definition.rs:44-47`), and `nearmiss.rs` reports `policy=Structural` for the mutating tool and `policy=Tolerant` for the `read_only` tool.
- **Structural:** `z` beside `a`, `z` alone, `z` beside `a`+`b`, and `c` beside `a`+`b` all give `Unresolved { UnknownParameter }`.
- **Tolerant renames:** `z` beside `a` is renamed to the free `b`, and `z` alone to `a` (`json.key.match`, `BestGuess`).
- **Tolerant drops:** `z` beside `a`+`b` (nothing free) gives `tool_call.parameter.drop-unknown` at `/inner/z` (`BestGuess`).

These agree with core's own scoping (`resolve.rs:48-53`; `docs/tool-calls/README.md:225`). The text no longer overstates the structural guarantee for tolerant tools.

Advisory, non-blocking: "when no declared name is free" really means "when no declared name the key nearly matches is free". This echoes the wording I suggested in Recheck 1. `nearmiss2.rs` shows the difference: `Inner { alpha, #[param(default = 0)] omega }` on a `read_only` tool, with `{"alpha":1,"alpah":2}`, gives `BestGuess` with `drop-unknown`, even though `omega` is free. Both possible outcomes (a rename as a guess, or a recorded drop) are stated correctly; only the rule that chooses between them is loose. Optional refinement: "renamed to a free declared name it nearly matches".

**RELEASE.md.** Lines 28-30 now say "a core patch release must not remove or change a `#[doc(hidden)]` support item that same-minor derive output uses". With the existing clause on additions, this covers both skew directions within a minor series. The marker test (`derive_support.rs:1028-1041`) and the closing sentence are unchanged.

**PR #19 body** (`gh pr view 19 --repo Blackcat-Informatics/purrassist --json body`, headRefOid `4d8a48e`):
- The "6cbb628, 4d8a48e" row states the per-policy near-miss rule in the same terms as the docs.
- Validation names head `4d8a48e` and explains the docs-only reuse of the `d0a99e9` qualification, which is confirmed above.
- "105 derive doctests (50 of them `compile_fail`)" matches `--list` (105) and the two suites (55 + 50).
- The exit-audit bullet names the near-miss policy scope as fixed in `6cbb628` and `4d8a48e`.
- The rest, unchanged since Recheck 1, still holds: the preflight wording, "Drop with recorded loss", the whitespace collapse, the G7 bridge-span disclosure, the residual risks, and "Hosted CI did not run."

Nit, non-blocking: the Behaviour bullet for "Drop with recorded loss" still says undeclared members "are dropped" without qualification. The changes row directly above it carries the near-miss qualification.

**Open findings: none blocking.**
- **MEDIUM-1:** discharged in Recheck 1.
- **LOW-1:** re-rated MEDIUM in Recheck 1; discharged here.
- **LOW-2, LOW-3:** discharged in Recheck 1. The LOW-2 advisory is discharged here.

Two advisory nits remain, both optional:
- the "nearly matches" wording;
- the PR Behaviour bullet.

**Every issue #15 acceptance criterion and every remediation item has execution evidence.** Limits (recorded above, all external):
- hosted CI did not run;
- the crates.io name reservation and trusted publisher are maintainer steps;
- the G8 `.deficiencies` record belongs to Stage 3.

VERDICT: PASS
