# Plan enhancement review

Reviewed plan: `/home/paudley/Active/purrassist/.worktrees/1-complete-purrassist-0-1-0-plan-and/.stage/complete-purrassist-0-1-0-plan-and/plan.md`

Inputs read: `issue-analysis.md`, `prior-art-assessment.md`, `.goals`, `AGENTS.md`, and the
worktree at `f11b45e` (manifests, `crates/host/src/tools/registry.rs`, `crates/host/src/tools.rs`,
`crates/host/src/mcp.rs`, `crates/cli/src/commands.rs`, `crates/cli/tests/protocol.rs`, the crate
READMEs, `scripts/check-*.py`, `vendor/tooling.json`, `.gitattributes`,
`scripts/data/verify-published/mcp-responses.jsonl`, `docs/tool-calls/RULES.md`).

Scope respected: nothing below touches hosted CI, CI-provider accounts, tagging or publishing.
No item conflicts with `.goals`; item 1's Rust-test route is the one `.goals` RUST-FIRST prefers.

## The concept the issue under-asks for

The issue asks for release docs and a README. The plan answers with a **fact checker**: prose is
still hand-written, and a Python script re-derives counts and sentences to catch drift. That is
the right diagnosis (prior-art section 6: "the checks pin sources of truth, not their copies in
prose") but only half the cure. The concept this feature is a shadow of is **documentation as a
projection**: every structural fact the public artifacts restate (the tool reference, crate map,
dialect list, fuzz-target list, version, supported-versions row, MCP server manifest, client
config) is already a machine-readable value in the repository, and the repository already has
the generate-then-pin pattern for exactly this (`scripts/generate-unicode.py` plus
`vendor/unicode/generated.json` digests; `check_derive_example` byte-equality of a marker block;
`docs/tool-calls/RULES.md` pinned to `rules::RULES` by a Rust test; the CLI `help()` splicing
`DialectId::BUILT_IN` into `{dialects}`). Structural content should be generated into marker
blocks and compared byte-for-byte; only narrative stays hand-written. Items 1 and 2 are that
transformation. Items 3 to 10 are leverage, utility and robustness on top of it.

## Items

### TRANSFORMATION

1. **Generate `docs/TOOLS.md` (and the other structural blocks) from the registry; check by
   byte equality, not by sentence matching.**
   - *Enhancement.* `crates/host/src/tools/registry.rs` is already the single declarative
     source for the CLI `call` command, MCP `tools/list`/`tools/call` and `Tools::definitions()`
     ("there is no second, hand-maintained dispatch path", registry.rs:3-10). A hand-written
     31-entry `docs/TOOLS.md` would be exactly that second path, for documentation. Instead:
     a Rust test (precedent: the RULES.md pinning test in
     `crates/core/tests/tool_call_rules.rs`) renders Markdown from `Tools::definitions()` for
     every entry: name, description, `annotations` (readOnlyHint/destructiveHint rendered as
     "read-only" / "mutating"), a parameter table derived from `inputSchema` (`required`,
     `enum`, `minimum`/`maximum`, `oneOf` arms), the output-shape family
     (`ASSIST_RESULT_OUTPUT`, `PLANNED_OUTPUT`, `COMMIT_OUTPUT`, array results), and the uniform
     CLI form `purrassist call NAME --root DIR < arguments.json` stated once. The test asserts
     the committed file equals the rendering (`make test` fails on drift). Hand-written per-tool
     narrative, the Rust entry point and the example live in a sidecar the generator merges
     (`docs/tools/<name>.md` or one JSON map); the generator requires the sidecar key set to
     equal the registry name set, so a new tool fails the gate until it is documented. The
     plan's "tool census against the committed `tools/list` fixture" becomes a side effect.
   - *Examples without 31 new hand-written files.* `crates/cli/tests/protocol.rs` already holds
     exactly one deterministic CLI/MCP equivalence fixture per registry entry
     (`fixtures()`, protocol.rs:214-218). Export each fixture's arguments and the recorded
     response (the test can write `docs/tools/examples/<name>.{request,response}.json` under a
     `--capture` style flag, as `verify-published.py --capture` does) so every example in
     TOOLS.md is a real executed request/response, not prose. Today `examples/` has 11 files;
     the plan's "an example file" per tool implies 20 more hand-written ones.
   - *Same pattern for the other restated facts.* Marker blocks
     (`<!-- generated:crate-map:begin -->` etc.) in README, RELEASE.md, QUALIFICATION.md and
     CHANGELOG for: the crate map (from `cargo metadata --no-deps` + `RELEASE_ORDER` + each
     manifest's `description`), the dialect list (from `DialectId::BUILT_IN`, which the CLI
     help already does), the fuzz-target list (from `scripts/fuzz.py TARGETS`), the workspace
     version, and the SECURITY supported-versions row. `scripts/generate-docs.py --check` (or
     the Rust test for Rust-sourced facts) compares blocks byte-for-byte. This replaces the
     plan's normalization of "ten"/"10" and matching of "the README dialect sentence", which
     the issue-analysis risk table itself calls out ("A new doc validator is too
     phrase-specific and brittle"). Prose that merely refers to a count should link to the
     generated block rather than restate it (prior-art recommendation 2, "Restate less").
   - *Cost.* L for the full set; M for TOOLS.md alone. The host crate needs no code change for
     the Rust-test route (`Tools::definitions()` is public, tools.rs:53).
   - *Payoff.* Decisive: it removes the drift class rather than detecting instances of it.
     The same fuzz count drifted twice and 11 commits fixed stale docs (prior-art section 5).
     The next three issues in this area (dialects #12-#14, any new tool, any new crate) then
     need zero edits to structural docs. It also keeps RUST-FIRST: the Rust test, not a Python
     string matcher, is the authority for Rust-sourced facts.
   - *Where it changes the plan.* Task 1 "Checker scope: facts are taken from their sources,
     never hard-coded" (keep the principle, change the mechanism from check-prose to
     generate-block); Task 2 "**docs/TOOLS.md (new):** the per-tool reference moves out of the
     README ... For each of the 31 tools: MCP name, `purrassist call` form, Rust entry point
     (crate path), arguments, outcomes and an example file"; Completeness contract row 3
     ("Every tool name in the `tools/list` result ... has exactly one entry in
     `docs/TOOLS.md`").

2. **Add the MCP interface's own manifest, `server.json`, as repository metadata.**
   - *Enhancement.* The user wants the MCP server presented as an equal interface. The crates
     interface has registry records, a release order and a preflight; the MCP interface has
     nothing analogous. The MCP ecosystem's discoverability unit is the registry `server.json`
     (schema `https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json`),
     and its `packages[].registryType` enum includes `cargo` (crates.io only), so a stdio server
     installed with `cargo install purrassist-cli` is directly representable:
     `name: io.github.Blackcat-Informatics/purrassist`, `version` = workspace version,
     `packages: [{registryType: "cargo", identifier: "purrassist-cli", version: "0.1.0",
     transport: {type: "stdio"}, packageArguments: [mcp, --root <dir>]}]`. `check-docs.py`
     asserts structurally that the version equals the workspace version, the identifier is in
     `RELEASE_ORDER`, and the arguments match the CLI usage line. The README's generic
     `mcpServers` client-config block (item 6) then becomes a projection of this file.
     Publication to the MCP registry is a later owner step (it needs GitHub OIDC and a
     published crate) and is **not** proposed for this stage; list it in the Task 6 handoff
     beside the tag, exactly as the derive trusted publisher is listed.
   - *Cost.* S-M. *Payoff.* The three-interface framing gets a machine-readable artifact for
     each interface (Cargo manifests, `server.json`, CLI `--help`), and MCP discoverability
     after release is a one-command owner step instead of new work.
   - *Where.* Task 2 item 3 MCP bullet; Task 6 handoff list.

### LEVERAGE

3. **Name the CLI≡MCP evidence now instead of deferring it.** The plan says "cite the CLI≡MCP
   test that exists in the host and cli tests; the implementer locates and names it". It is
   `every_registry_entry_has_a_fixture_validated_through_cli_and_mcp` and
   `mcp_tools_list_names_equal_the_registry` in `crates/cli/tests/protocol.rs` (lines 743 and
   792), plus `generate_json_witness_agrees_across_cli_and_mcp_for_every_outcome`. Cost S.
   Payoff: removes an unknown from Task 2 and these are the fixtures item 1 reuses.

4. **Make the three-interface identity claim exact and executed.** The README section will
   show one temperature decode via Rust, MCP and CLI and say they return identical output. The
   `purrassist decode` subcommand prints `schema.recover(...).to_node()` directly
   (commands.rs:108-109), while `decode_json` through `call`/MCP goes through `Tools::call`;
   only `call` vs MCP identity is tested today. Have `test-check-docs.py` run the README's
   three snippets on `examples/temperature.*` and assert the `value` (and, if true, the whole
   result) is identical across `decode`, `call decode_json`, and MCP `tools/call decode_json`;
   if the `decode` shape differs, state the difference in the README instead of claiming
   identity. Cost S. Where: Task 2 item 3 and its "Focused check".

### UTILITY

5. **Crate metadata for discoverability, not just two descriptions.** Facts at `f11b45e`:
   - All five library crates share keywords `["ai", "json", "recovery", "matching", "agents"]`
     (derive swaps `matching` for `derive`). None says `mcp`, `mcp-server`, `tool-calls`,
     `json-schema`, `markdown` or `llm`, so crates.io keyword search for the MCP server finds
     nothing. Align per-crate keyword sets (max 5 each) with the Task 5 GitHub topics, e.g. cli
     `mcp`, `mcp-server`, `cli`, `tool-calls`, `llm`; core `json-schema`, `json-repair`,
     `fuzzy-matching`, `markdown`, `wasm`.
   - `purrassist-cli` is categorized `parsing`/`text-processing`; `command-line-utilities` is
     the slug a CLI is browsed under.
   - `crates/cli/Cargo.toml:15` sets `documentation = "https://docs.rs/purrassist-cli"` but its
     only target is the binary with `doc = false` (line 27), so docs.rs will show no content.
     Either point `documentation` at the repository tool reference, or drop `doc = false` and
     add `#![doc = include_str!("../README.md")]` to `main.rs` so docs.rs renders the CLI
     README (and doctests its code blocks, which `make check` then covers).
   - No `homepage` is set anywhere; crates.io shows it as a distinct link. Set it in
     `[workspace.package]`.
   - `purrassist-host` has a `cfg(windows)` dependency and Windows durability code; a
     `[package.metadata.docs.rs] targets` list that includes `x86_64-pc-windows-msvc` makes
     docs.rs document that path.
   - `check-release.py` reads none of these fields today (grep: no `keywords`/`categories`/
     `readme`/`homepage`); add the assertions (keyword count ≤ 5, cli keywords include `mcp`,
     `documentation` URL consistent with the chosen route) there, since it already walks the
     manifests.
   Cost S. Payoff: the crates.io and docs.rs landing pages are where the "Rust crate" interface
   is discovered; the plan currently leaves them with pre-public metadata. Where: Task 2
   "Cargo descriptions: ... Other crates are unchanged unless a reviewer finds a factual
   defect" (this is the factual defect).

6. **MCP client configuration as a tested artifact, with the `.purrassist` side effect
   disclosed.** `Tools::new` calls `Store::open(root, …)` and "creates `.purrassist`"
   (tools.rs:40-47), so `purrassist mcp --root <dir>` writes a journal directory into the
   user's workspace on first use. The README MCP section must say so and recommend a VCS
   ignore entry; an MCP user who did not read `purrassist-host` will not expect it. Provide the
   generic `mcpServers` JSON plus the two most common concrete forms with placeholder paths
   only (VS Code `.vscode/mcp.json` using `${workspaceFolder}`; a Claude Code
   `claude mcp add purrassist -- purrassist mcp --root <dir>` one-liner), and have
   `test-check-docs.py` read the generic JSON out of the README fence, substitute the built
   binary for `command`, and run the initialize/`tools/list` round trip the plan already
   scripts, so the documented config is the executed config. Cost S. Where: Task 2 item 3
   "a generic stdio MCP client config JSON with no user-specific paths".

7. **Derive the known-limitations issue set; do not hard-code it in the checker.** Task 3's
   rule "its Known limitations names #6, #7, #10, #11, #12, #13, #14, #16, #17 and #18"
   contradicts Task 1's own principle ("facts are taken from their sources, never
   hard-coded"), and the issue-analysis records the states of #12-#14 and #16-#18 as UNKNOWN.
   Rule instead: collect every `#N` the public docs cite as tracked/open/known (the
   "Still tracked" lists in README, IMPLEMENTATION, QUALIFICATION, `docs/tool-calls/README.md`,
   and the FUZZING/RESOURCES defect citations) and require that set to be a subset of the
   CHANGELOG known-limitations section; record a dated `gh issue list --state open --json
   number,title,state` observation in the task notes (and resolve the open question before the
   lists are finalized), the same way the plan records registry state as a dated observation.
   Cost S. Where: Task 3 "check-docs.py rules" and Completeness contract row 6.

### ROBUSTNESS

8. **Packaged-README link rules.** crates.io rewrites relative links against `repository`;
   docs.rs does not. Every packaged README today uses absolute GitHub URLs, and Task 2 adds
   "a link to the repository CHANGELOG" to all six. Add a `check-docs.py` rule that packaged
   READMEs contain no relative links or images, and that the CHANGELOG link is stable across
   publication (`blob/main/CHANGELOG.md`, or the tag once it exists). Separately, the derive
   README's docs.rs `latest` links resolve to the 0.0.0 placeholder until publication
   (issue-analysis risk table); pin them to `/0.1.0/` or say so. Cost S. Where: Task 2
   "Crate READMEs"; Task 1 "Links and SPDX".

9. **Decide the binary-asset licensing convention now, and verify the renderer.** Task 4 says
   "`.gitattributes` or REUSE-style registration as the repo already does for binaries; the
   implementer checks". There is no precedent: `.gitattributes` only marks `*.png binary` and
   the SPDX scan covers `.rs .py .js .mjs .toml .sh` under `crates/` and `scripts/`. Decide:
   REUSE `docs/social-preview.png.license` sidecar, and extend the SPDX scan (which Task 1
   already extends to `.md`) to require a sidecar for every tracked binary. Also have the
   implementer confirm `rsvg-convert` renders the logo's `feDropShadow` filter
   (`docs/purrassist-logo.svg:8-9`) rather than silently dropping it, since the preview reuses
   that artwork; the plan's text-to-paths rule covers fonts but not filters. Cost S. Where:
   Task 4 "Registration" and "Focused check".

10. **CHANGELOG headings that survive the tag without a doc edit.** The plan's
    `## 0.1.0 — unreleased` heading "(the date is set at tagging)" puts a documentation edit
    inside the separately authorized, irreversible tag step, and the em-dash form is irregular
    for the parser `check-docs.py` must contain. Use Keep a Changelog structure
    (`## [Unreleased]` and `## [0.1.0] - YYYY-MM-DD`), accept the unreleased section as the
    0.1.0 body while untagged, and let the tag step be the only thing that renames the heading
    (a mechanical, checkable change). Cost S. Where: Task 3 "CHANGELOG.md (new, SPDX)".

## Items the plan already handles well (no action)

The fact checker wired into `make hygiene` with negative tests, the dated-observation wording
for registry state, the `Refs #1` discipline, the social-preview digest check verifying the SVG
source rather than PNG bytes, and the declined combined `LICENSE` file are sound and need no
change. Nothing here conflicts with `.goals`; no item is **REJECTED**.

## Summary (pass 1)

The plan fixes today's stale facts and adds a checker; it does not yet make the documentation a
projection of the registry and manifests it describes. Item 1 is the transformational change
(generate TOOLS.md and the structural blocks from `Tools::definitions()` and the manifests, pin
by byte equality through a Rust test); item 2 gives the MCP interface its own manifest; items
5-7 close concrete metadata and derivation gaps the plan leaves open.

Pass 1 verdict: BLOCKED

---

# Pass 2 (recheck of plan revision 2)

Reviewed: the same `plan.md`, now with "Review dispositions (revision 2)", the rewritten
Completeness contract (rows at lines 143-158), the rewritten Tasks 1-6, and the extended
"Enhancements considered and declined" section (lines 166-179).

## Disposition of pass-1 items

1. **Generated TOOLS.md — DISCHARGED.** Adopted concretely: `tools_reference_matches_registry`
   in `crates/cli/tests/protocol.rs` renders the marker block from `Tools::definitions()` in
   registry order, asserts byte equality with a diff, and blesses under the `recovery_goldens`
   precedent (contract row "Tool reference is a generated projection of the registry"). The
   per-tool heading census is a consequence of generation, as proposed. Examples are executed
   rather than hand-written (`examples_run_through_cli_and_mcp`, orphan-free `EXAMPLES` table,
   output-schema-valid in both transports). The partial adoption for prose counts ("structural
   lists ... are generated or sourced; free prose numbers stay checked facts") is a sound
   boundary: the dialect list now has a Rust assertion plus a stem check, the fuzz-target table
   is one row per `TARGETS` entry, the crate map is set-equal to `RELEASE_ORDER`, and the
   known-limitations set is a single JSON source. Only the bare fuzz-count word remains a
   normalized prose fact, which is acceptable.
   One non-blocking note on Design A5 ("the Rust entry-point column is dropped from the
   generated block"): for the three-interface promise to hold inside TOOLS.md, the generated
   block should state once, above the entries, the uniform Rust form every tool has
   (`purrassist_host::tools::Tools::new(root)?.call("NAME", &args)` and
   `Tools::definitions()`); that is a true, rustdoc-resolvable statement and needs no per-tool
   column.

2. **`server.json` MCP manifest — STILL-OPEN (narrowed).** The decline reason is "its
   correctness can only be proven by registry acceptance, which is publication and out of this
   stage. An unvalidated manifest would be a claim without evidence." Two parts of that are
   not compelling, and one consequence is missing:
   - The manifest's shape is provable offline: the schema is a public JSON Schema
     (`https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json`), and the
     repository ships its own Draft 2020-12 validator (`purrassist validate --schema`). A
     vendored, digest-pinned copy of the schema plus a `validate` run in `check-docs.py` is
     executable evidence of exactly the kind AGENTS.md asks for. Deferring the file itself is
     nevertheless a defensible choice and I accept it as a post-release option.
   - **The consequence the decline misses is irreversible.** The registry verifies ownership
     of `cargo` packages by looking for a visible `mcp-name: <server name>` token in the crate
     README **as rendered by crates.io**, and crates.io strips HTML comments, so the token must
     be visible Markdown text (the documented pattern is one bullet, e.g.
     `- MCP Registry name: \`mcp-name: io.github.<owner>/purrassist\``). crates.io versions are
     immutable. If `purrassist-cli` 0.1.0 is published without that line, the MCP server can
     never be registered against 0.1.0; it would need a 0.1.1 whose only change is a README
     line. The plan's Task 2 is the one place where the packaged `crates/cli/README.md` is being
     finalized for exactly this reason ("That text would become permanent in the crates.io
     archives").
   - *Remedy (S cost, one line plus one check):* add the visible `mcp-name:` bullet to
     `crates/cli/README.md` in Task 2, with the server name derived from the repository owner
     login (`io.github.<github-owner>/purrassist`; the implementer uses the exact login the
     registry's GitHub namespace expects, not a display name), and a `check-docs.py` rule that
     the packaged cli README contains the token as visible text (not inside an HTML comment).
     Keep `server.json` and the registry publication in the handoff as the post-release owner
     option, as the plan already does. This proposes no publication.
   Sources (general MCP registry documentation only):
   https://modelcontextprotocol.io/registry/package-types

3. **Named CLI≡MCP tests — DISCHARGED.** The README cites
   `every_registry_entry_has_a_fixture_validated_through_cli_and_mcp` (contract row "README
   presents three equal interfaces").

4. **Decode identity test — DISCHARGED (decline is compelling).** `decode` and `decode_json`
   are deliberately different shapes (recovery result with exit codes vs the registry output
   schema); the plan now documents `decode`/`match`/`validate` as shortcuts with their own
   output, claims no identity, and exercises the README temperature request through
   `decode_json` in both transports. That is exactly the alternative pass 1 asked for.

5. **Crate metadata — DISCHARGED.** `homepage` set; cli gets `command-line-utilities` and
   `development-tools`, keywords `mcp, cli, json, recovery, agents`, and a documentation link
   to the repository (not the empty docs.rs page); host gains `mcp`; `check-docs.py` asserts
   the cli documentation link is not docs.rs. The docs.rs `targets` note for host was not
   taken up; non-blocking.

6. **MCP client config and `.purrassist/` disclosure — DISCHARGED.** The README states that
   MCP creates `.purrassist/` journals in the root; the config JSON is parsed and tied to the
   CLI usage (`command` = `purrassist`, `args` = `["mcp","--root",…]`). The live round trip now
   lives in the Rust `examples_run_through_cli_and_mcp` test instead of a script, which is a
   better home.

7. **Derived known-limitations set — DISCHARGED.** `docs/known-limitations.json` is the single
   source (number, title, impact, `blocks_release: false`), equal-set checks against
   QUALIFICATION, FUZZING and the CHANGELOG, and the issue states were observed on 2026-10-12
   (all open), resolving the issue-analysis open question.

8. **Packaged-README links — DISCHARGED.** Packaged READMEs are derived from publishable
   `cargo metadata` packages, must have no `docs.rs/.../latest` link, and derive's links are
   pinned to `0.1.0`. The explicit "no relative links in packaged READMEs" rule is not
   named; today none exist and the global relative-link rule covers resolvability, so this is
   non-blocking.

9. **Binary-asset licensing and renderer verification — DISCHARGED.** REUSE sidecar
   `docs/social-preview.png.license`, SPDX scan accepts sidecars, `docs/assets.json` pins SVG,
   PNG, tool version and font digest, Noto Sans (OFL-1.1) and `rsvg-convert` registered with
   notices, and `feDropShadow` rendering verified visually. Stronger than requested.

10. **CHANGELOG headings — DISCHARGED.** Keep a Changelog `## [Unreleased]` and
    `## [0.1.0] - unreleased`, with the date set at tagging as a mechanical rename.

## New design introduced in revision 2

11. **Migration test frozen by `git show f11b45e:README.md` — new, S cost, non-blocking.**
    Contract row "No README claim is lost in the move" reads the pre-move README from git
    history inside `test-check-docs.py`, which `make hygiene` runs on every `make check`. That
    makes the gate depend on commit `f11b45e` being present in the clone; a shallow clone of a
    later `main` would fail hygiene for a reason unrelated to the tree. Freeze the extracted
    token-and-link set once into a committed fixture (`scripts/data/readme-claims-f11b45e.json`,
    with the source commit recorded inside it) and compare against that, so the check is a
    pure function of the tree. Where: contract row at line 149; Task 2 "plus the migration
    test".

Everything else introduced in revision 2 (`docs/assets.json`, the `PURRASSIST_BLESS` guard, the
explicit banned-phrase scope, the Task 5 NOT DELIVERED rule, the homepage switch listed for the
handoff) is concrete and consistent with `.goals`.

## Pass 2 summary

Ten of ten pass-1 items are adopted or soundly declined except item 2, whose decline rests on an
incomplete reason: the MCP registry's `cargo` ownership check reads a visible `mcp-name:` token
from the crates.io-rendered README, and 0.1.0's README is immutable once published. The fix is
one visible line in `crates/cli/README.md` plus one checker rule in Task 2; `server.json` and
registration stay in the handoff. Item 11 is a small robustness note on the new migration test.

Pass 2 verdict: BLOCKED

---

# Pass 3 (final recheck of plan revision 3)

Reviewed: `plan.md` revision 3, specifically the new contract row at line 151 ("MCP Registry
eligibility of the published 0.1.0 CLI"), the rewritten row at line 149 ("No README claim is
lost in the move"), the amended decline at lines 172-175, Task 2 line 248 and its focused check
at line 256, and the "Enhancement, pass 2" dispositions at lines 365-368.

## Disposition of the two items left open after pass 2

2. **`mcp-name:` token / `server.json` — DISCHARGED.** Adopted as narrowed, and concretely:
   `crates/cli/README.md` gets a visible Links bullet
   `` MCP Registry name: `mcp-name: io.github.Blackcat-Informatics/purrassist` ``; `check-docs.py`
   requires the token outside any HTML comment and code fence in the packaged cli README, with
   failing fixtures for a missing token and a commented-out token (contract row, line 151).
   The `server.json` file and registry submission are recorded in RELEASE.md and the handoff as
   post-publication owner steps, and the decline text now states the irreversible prerequisite
   explicitly (lines 172-175). Nothing here publishes anything.
   I also checked the one residual risk pass 2 flagged, the namespace casing: the registry
   compares the `io.github.<login>` namespace against the authenticated GitHub login verbatim
   and case-sensitively (lowercasing the login is a documented cause of a 403 at publish time).
   The plan's `Blackcat-Informatics` is the exact owner segment of the workspace `repository`
   URL (`https://github.com/Blackcat-Informatics/purrassist`), so the immutable line is right.
   Non-blocking refinement for the implementer: have the `check-docs.py` rule derive the
   expected namespace from the `repository` URL's owner segment rather than a literal, so the
   token is a sourced fact like the other checked facts.

11. **Migration check and `git` in the gate — DISCHARGED.** The check is now a one-time Task 2
    focused check, recorded in `validation.md` and the Task 2 review, with no `git` invocation
    in `make check` (row at line 149; Task 2 focused check, line 256). The gate is a pure
    function of the tree, which is what pass 2 asked for; freezing the token set into a
    fixture is unnecessary once the check leaves the gate.

## Previously discharged items

Items 1, 3-10 remain DISCHARGED; revision 3 does not change their adopted mechanisms. The two
non-blocking notes from pass 2 (the uniform Rust form stated once above the generated TOOLS.md
block; docs.rs `targets` for host) stay optional and do not affect the verdict.

## New design introduced in revision 3

None beyond the two rows above. The revision-3 row text is specific enough to implement
without further decisions (exact bullet text, exact rule scope, named failing fixtures, where
the post-publication steps are recorded).

## Pass 3 summary

Every pass-1 item is adopted or soundly declined, the narrowed item 2 lands the irreversible
README prerequisite inside this stage while keeping registration out of it, and the migration
check no longer couples `make check` to git history. The plan is maximal for this stage's
scope.

Sources used in this pass (general MCP registry documentation and public issue reports on
namespace casing only, no repository content):
https://modelcontextprotocol.io/registry/package-types,
https://modelcontextprotocol.io/registry/authentication,
https://github.com/AindriuB/data-prism/pull/68,
https://github.com/JonasHeinickeBio/pyEuropePMC/pull/208

VERDICT: PASS
