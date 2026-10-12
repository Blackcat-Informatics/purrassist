# Plan design review (SOLID / ETHOS)

Reviewed plan: `/home/paudley/Active/purrassist/.worktrees/1-complete-purrassist-0-1-0-plan-and/.stage/complete-purrassist-0-1-0-plan-and/plan.md`

Code read (worktree `f11b45e`):

- `scripts/check-repository.py` (whole file; `main()` 294-386, `check_derive_example` 254-291, SPDX scan 303-311, generated-digest loops 329-340, tooling loop 374-377)
- `scripts/test-repository-audit.py` (loader 15-24, `DeriveExample` 202-241)
- `scripts/check-release.py` (`RELEASE_ORDER` 19-26, `main` 210-247)
- `scripts/test-check-release.py:28-31`, `scripts/test-fuzz.py:229-249`, `scripts/test-verify-published.py:428-480`
- `scripts/fuzz.py:1054-1093` (`TARGETS`)
- `scripts/verify-published.py:49,203,235,371`
- `scripts/data/verify-published/mcp-responses.jsonl` (name census by grep), `mcp-requests.jsonl:3`
- `Makefile` (hygiene 11-18, fuzz loop 62)
- `vendor/tooling.json`, `vendor/unicode/generated.json`, `.gitattributes`
- `crates/host/src/tools/registry.rs:1-140` (`ENTRIES`, handler shape)
- `crates/cli/tests/protocol.rs:743-842` (CLI/MCP parity, `tools/list` equals registry)
- `crates/core/src/tool_call/dialect.rs:199-212,267-270`, `crates/core/src/contract.rs:101`
- `crates/core/tests/tool_call_dialects.rs:227-245`, `crates/cli/tests/tool_calls_cli.rs:1166-1184`
- `docs/tool-calls/dialects/` listing (12 files), `README.md` fact lines 106, 387

## Overall shape

A new `scripts/check-docs.py` with its own `scripts/test-check-docs.py`, wired into `hygiene`, is
the right boundary. `check-repository.py::main()` (lines 294-386) is already one 90-line function
that mixes attribution, frozen-data, dependency, and derive-example checks; its reason to change is
"dependency and provenance policy". Doc-fact rules have a different reason to change (every feature
PR) and different inputs (prose). Growing `main()` further would make every doc rule share one
undifferentiated error list and one `SystemExit`. The split follows the existing one-script-one-test
convention (`check-release.py`/`test-check-release.py`, `fuzz.py`/`test-fuzz.py`). Loading sibling
scripts with `importlib.util.spec_from_file_location` is the established seam
(`test-check-release.py:28`, `test-repository-audit.py:15-24`, `test-fuzz.py:17`); `check-release.py`
and `fuzz.py` guard `main` and have no import side effects, so importing `RELEASE_ORDER` and
`TARGETS` is robust.

The tool census source is sound: `mcp-responses.jsonl` is replayed against the freshly built
workspace CLI in `hygiene` (`test-verify-published.py:451-480`), and `tools/list` is pinned to
`ENTRIES` by `protocol.rs:792-840`, so the fixture cannot be stale while the gate is green.

The findings below are where the described design would not deliver the invariant it claims.

## BLOCKING

### B1. The dialect check's named source does not contain the values it compares

Plan line 160: "**Built-in dialect ids:** taken from `DialectId::BUILT_IN` source and compared with
the README dialect sentence."

`BUILT_IN` (`dialect.rs:199-212`) lists variant names (`Self::AnthropicToolUse`, ...). The README
sentence (`README.md:106`) lists ids (`qwen3-coder-xml.v1`, `hermes.v1`, ...). The mapping is two
hops away: `fn id()` at `dialect.rs:267-270` matches each variant to a `const` (`ANTHROPIC_TOOL_USE.id`)
whose string literal lives in a third place. A Python regex over `BUILT_IN` yields 12 CamelCase
names and nothing to compare with the README. The implementer would either hard-code a
variant-to-id table in the checker (the hand-copied fact the task exists to remove; adding a 13th
dialect then requires editing the checker, and forgetting to does not fail) or write a three-construct
Rust parser.

The repository already has the right seams: `docs/tool-calls/dialects/*.md` stems are exactly the 12
ids, and `crates/core/tests/tool_call_dialects.rs:227-245` already pins `BUILT_IN.map(id)` to a
literal list in a `make test` suite.

REWRITE TO: `check-docs.py` takes the id set from the `docs/tool-calls/dialects/*.md` stems and
asserts the README (and any other listed file) names exactly that set. Add one assertion in
`tool_call_dialects.rs::all_registered_dialects_have_anchor_fixtures` that the sorted stems of
`docs/tool-calls/dialects/` equal `DialectId::BUILT_IN.iter().map(DialectId::id)`. That closes
source -> docs with no Rust parsing in Python and no second copy of the list.

### B2. The social-preview digest check does not catch the stale PNG it claims to catch

Plan lines 142 and 279: "A checked digest of the SVG catches a stale PNG. ... the check deliberately
verifies the source digest and dimensions, not PNG bytes." / "the registered SVG digest equals the
current SVG."

With only the SVG digest recorded, the failure the check reports is "SVG changed, registry not
updated". The error message will tell the editor to update the digest; once they do, an un-re-rendered
PNG (still 1280x640, still under 1 MB) passes. That is the stale-artifact case, and the design lets
it through silently. The repository's own precedent pins the *output* bytes, not the input:
`vendor/unicode/generated.json` records the sha256 of each generated file and
`check-repository.py:329-332` fails with "regenerate and review" when they differ.

REWRITE TO: the registry entry records `svg_sha256`, `png_sha256`, the exact render command, and the
`rsvg-convert` version used; `check-docs.py` asserts both digests plus IHDR dimensions and the size
bound. Rendering-byte variance across `rsvg-convert` versions is handled the same way as the Unicode
tables: a tool upgrade is a deliberate regenerate-and-review commit that updates the recorded version
and digest. To make the render reproducible under that rule, drop the "or uses a generic font family
with a recorded fallback" option (line 273): a font fallback resolves per machine, so the pinned PNG
digest would fail on a second machine for a non-defect. Convert text to paths and inline the logo
artwork rather than `<image href>` (rsvg's external-resource policy differs by version).

## ADVISORY

### A1. Two marker-block copy checkers in two scripts

Plan line 119/191: "a byte copy of the facade README block checked like the derive example (an
extended `check_derive_example` pattern)", while every other README structural rule (the
`three-interfaces` marker, crate map, counts) lives in `check-docs.py`.

If the Rust snippet copy is checked by extending `check_derive_example` in `check-repository.py`
(254-291) and the enclosing `three-interfaces` marker is checked in `check-docs.py`, the README's
critical "doctested copy equals shown copy" guarantee is implemented twice in two files with two
test suites, and the next copied block (a CLI README snippet, say) has no obvious home.

REWRITE TO: generalise `check_derive_example` into one `check_doc_copies(root, blocks)` taking a list
of `(marker, copies, doctested)` tuples, keep it in one script (either leave it in
`check-repository.py` and have `check-docs.py` import it, or move it to `check-docs.py` and delete
it from `check-repository.py`), and register both `derive-example` and the new Rust block there.
`test-repository-audit.py::DeriveExample` moves with it.

### A2. The "Pre-release" rule starts as a hand-listed allowlist

Plan line 162: "the rule lands now and initially lists only files fixed in this task", then line 213:
"extended to all six".

A list of README paths inside the checker is the same hand-copied set the rule is meant to replace;
a seventh crate would not be covered until someone remembers the list. The set is derivable:
`RELEASE_ORDER` (already imported) maps to `crates/<dir>/README.md` via each manifest's `readme`
field, or `cargo metadata --no-deps` `readme` paths (as `check-release.py:214` already runs).

REWRITE TO: move the rule to Task 2 (where the READMEs are fixed) and derive the file set from the
publishable packages' `readme` fields; no path list in the checker.

### A3. Fact regexes must fail when the sentence is absent

Plan line 158: "Prose counts are written as words or digits; the checker normalizes 'ten'/'10'."
The plan's negative tests (line 117) mutate a present-and-wrong count. The other failure mode is
rewording: if someone rewrites `README.md:387` so the count regex no longer matches, a checker that
iterates matches and asserts each one passes vacuously, and the guard is gone with a green gate.

REWRITE TO: each fact rule names its target files and requires at least one match per file
(zero matches is an error naming the file and the pattern); `test-check-docs.py` gains an
"absent sentence fails" fixture per rule. Prefer anchoring with HTML-comment markers (the
`derive-example` precedent) over free-text regexes where a fact is restated.

### A4. The census must pair by JSON-RPC id and parse `result.tools`, not grep names

Plan line 119: "Every tool name in the `tools/list` result of `scripts/data/verify-published/
mcp-responses.jsonl`".

Responses carry no `method`; the `tools/list` response is identified only by its id, which is in
`mcp-requests.jsonl:3` (`"id":2`). A name grep over the responses file returns 34 names today
(`answer` on line 10 and `list_dir` on lines 20-21 are tool-call recovery results, not tools), so a
naive census would demand bogus `TOOLS.md` entries.

REWRITE TO: read both files, find the request whose `method == "tools/list"`, take the response with
that id, and use `result.tools[].name`. `verify-published.py:235 read_lines` is reusable. State the
`TOOLS.md` entry convention the checker keys on (e.g. exactly one `### \`<name>\`` heading per
tool) so "exactly one entry" is decidable.

### A5. The per-tool "Rust entry point" column is an unverified restated fact

Plan line 203: "For each of the 31 tools: MCP name, `purrassist call` form, Rust entry point (crate
path)".

Handlers are closures and shared functions in `ENTRIES` (`registry.rs:75,83`: `super::embeddings::call`,
`markdown_handler`); the public Rust entry for every tool is `purrassist_host::Tools::call`. A
per-tool crate path written by hand is 31 new unchecked prose facts, which is the drift class this
stage is removing; nothing in the plan verifies the named paths exist.

REWRITE TO: either state the Rust interface once at the top of `TOOLS.md` (`Tools::call(name, args)`
plus the facade module where a direct API exists), or, if per-tool paths are kept, make each an
intra-doc link in a Rustdoc-included file so `make doc` (`-D warnings`) checks them.

### A6. Registering `rsvg-convert` needs a vendored notice, and the registry file should be named

Plan lines 276-277: "the repository's generated or binary asset registry (... the implementer finds
it)" and "`rsvg-convert` is registered with its version in `vendor/tooling.json`".

There is no generic asset registry: `vendor/unicode/generated.json` and `vendor/entities/generated.json`
are consumed by Unicode/entity-specific loops (`check-repository.py:329-340`) with Unicode-specific
messages. Every `vendor/tooling.json` entry must carry `notices` (`check-repository.py:374-377`
indexes `entry['notices']` unconditionally; an entry without it raises `KeyError`, not a clean error).

REWRITE TO: name the file in the plan (e.g. `docs/assets.json` or `vendor/generated-assets.json`)
and read it from `check-docs.py`, where the PNG check already lives; add the librsvg
(LGPL-2.1-or-later) notice under `vendor/licenses/` to the tooling entry, and cover both in
`docs/DEPENDENCIES.md`.

### A7. `.md` SPDX scanning duplicates the existing scanner

Plan line 161: "Tracked `*.md` must carry SPDX" in `check-docs.py`; `check-repository.py:303-311`
already scans `crates/` and `scripts/` for SPDX by suffix. Two scanners with different file sets and
header rules (first five lines vs. whatever the new one does) will disagree on a `.md` under `crates/`.

REWRITE TO: extend the existing scan (add `.md` to the suffix set and the repository root's tracked
`.md` files to its roots) rather than a second scanner.

## Not findings

- Fuzz count from `fuzz.TARGETS` cross-checked with the Makefile loop: `test-fuzz.py:242-244`
  already reads the loop line; asserting set equality subsumes it. Fine.
- `RECEIPT_VERSION` by regex: `contract.rs:101` is one anchored line; require exactly one match
  (A3 applies) and it is robust.
- Workspace version via `tomllib` on `Cargo.toml`: offline, single source; fine.
- The fixture-based tool census is a correct seam given the freshness test in `hygiene`.
- Task boundaries: rules landing with the facts they pin (Tasks 1-4 each touch `check-docs.py`)
  is acceptable; each task's focused check runs the negative tests.

VERDICT: BLOCKED

---

# Pass 2 (plan revision 2)

Additional code read: `crates/core/tests/recovery_goldens.rs:190-193`,
`crates/core/tests/tool_call_goldens.rs:886-889` (the `PURRASSIST_BLESS` / `CI` precedent),
`crates/core/tests/tool_call_rules.rs:1955-2048` (`rules_doc_matches_registry`, the RULES.md
projection precedent), `.github/workflows/verify.yml:21-25` and `release-qualification.yml:24-43`
(`actions/checkout` with no `fetch-depth`, then `make check`).

## Prior findings

- **B1 — DISCHARGED.** Contract row "Dialect ids have one source" (line 144) and Task 1 (line 188):
  `check-docs.py` reads the `docs/tool-calls/dialects/*.md` stems; a new assertion in
  `tool_call_dialects.rs` pins `BUILT_IN.map(id)` to those stems. Source -> stems -> prose, no Rust
  parsing in Python.
- **B2 — DISCHARGED.** Line 155: `docs/assets.json` records SVG sha256, PNG sha256, render command,
  `rsvg-convert` version and font digest; `check-docs.py` asserts both digests, IHDR and size, each
  with a failing fixture; a fresh render reproducing the PNG sha256 is a Task 4 focused check. Single
  font "Noto Sans" with no fallback and the logo inlined (line 300) removes the per-machine variance
  I named. The font file sha256 is a recorded observation (the checker cannot locate a system font
  offline); the contract correctly claims only the two image digests as checked.
- **A1 — DISCHARGED.** Line 148/250: one generalized marker-copy rule in `check-repository.py`
  covering `derive-example` and `three-interfaces`.
- **A2 — DISCHARGED.** Line 150: packaged READMEs derived from publishable `cargo metadata`
  packages; the rule lands in Task 2 with the fixes.
- **A3 — DISCHARGED.** Line 143: every fact regex fails on zero matches, with an absent-sentence
  fixture per rule.
- **A4 — SUPERSEDED (accepted).** The census now comes from `Tools::definitions()` inside
  `protocol.rs` (line 146), the same seam `every_registry_entry_has_a_fixture_validated_through_cli_and_mcp`
  uses (`protocol.rs:744`), following `rules_doc_matches_registry` (`tool_call_rules.rs:2031`). This
  is a better seam than the jsonl fixture: one hop, no id pairing.
- **A5 — DISCHARGED.** Line 375: the per-tool Rust column is dropped from the generated block.
- **A6 — DISCHARGED.** `docs/assets.json` named; `rsvg-convert` and Noto Sans registered in
  `vendor/tooling.json` with notices (line 155, 307).
- **A7 — DISCHARGED.** Line 189: the existing SPDX scan is extended to `*.md`.

## New text introduced by the fixes

### B3 (BLOCKING). The migration test puts a git-history dependency into `make hygiene`

Line 149: "`test-check-docs.py` has a migration test over the pre-move README text, frozen at
`f11b45e` and read with `git show`."

`test-check-docs.py` runs in `hygiene`, which runs in `make check` (`Makefile:6,11-18`). Both
workflows invoke `make check` after `actions/checkout` with no `fetch-depth`
(`verify.yml:21-25`, `release-qualification.yml:24-43`), which is a depth-1 clone; `git show
f11b45e:README.md` fails there, and in any developer `--depth 1` clone, so the gate fails for a
reason unrelated to the tree. Hosted CI being out of scope for this stage does not change that
`make check` is the documented local gate and must not depend on which commits a clone happens to
contain. The check is also a one-time migration assertion that would run on every gate forever.

REWRITE TO: freeze the token and link-target list from the `f11b45e` README into a tracked fixture
(`scripts/data/readme-claims-f11b45e.json`, generated once and committed in Task 2), and have the
migration test read that file. Alternatively run the migration check once in Task 2 as a focused
check recorded in `validation.md` and leave it out of `hygiene`. Either way, no `git` invocation in
`make check`.

### A8 (ADVISORY). Say which `### <tool>` headings `check-docs.py` counts

Line 146: "Hand-written per-tool narrative ... sits outside the block, under `### <tool>` headings.
`check-docs.py` requires exactly one heading per tool name in the generated block."

The generated block is already byte-pinned by `tools_reference_matches_registry`, so a second
structural check *of the block* is redundant. The useful check is the other direction: the
hand-written narrative headings outside the block must equal the tool names inside it (so a new
tool gets narrative, and `read_file`/`undo_file` cannot be dropped again). As written it is
ambiguous which headings are meant.

REWRITE TO: "`check-docs.py` requires exactly one `### <tool>` narrative heading outside the block
for every tool name in the generated block, and no heading for a name not in it."

### A9 (ADVISORY). The examples table must say how non-request files are handled

Line 147: "asserts that every `examples/*.json` is listed (no orphan)".

`examples/temperature.schema.json` is a schema, not a tool request, and `examples/temperature.input.txt`
is a text input; the orphan rule as written would require a tool mapping for the schema file. The
plan already exercises the temperature pair via `decode_json`, so this is a wording gap, but a
literal implementation would either exempt the file by hand or fail.

REWRITE TO: the `EXAMPLES` table maps each entry to `(tool, request-construction)`; the temperature
pair is one entry whose request is built from the schema and input files, and the orphan rule covers
every file under `examples/`.

## Not findings

- `docs/known-limitations.json` as the single source for QUALIFICATION, FUZZING and CHANGELOG sets:
  correct one-source design; set equality in three places is cheap and each has a fixture.
- `PURRASSIST_BLESS=1` with a `CI` guard follows `recovery_goldens.rs:190-193` exactly.
- The qualification-workflow summary checked against `run:` step names parsed from the YAML: text
  parsing of a file this repository authors, matching `validate_publish_workflow`'s approach.

VERDICT: BLOCKED

---

# Pass 3 (plan revision 3, final)

Reviewed only the text changed for B3, A8 and A9 (plan lines 146, 147, 149, 252, 362), verified
by reading the plan rather than the coordinator's summary.

- **B3 — DISCHARGED.** Line 149 now reads: "Task 2 runs a one-time migration check as a focused
  check and records it in `validation.md`; there is no `git` in `make check`." Line 252 lists it
  under the Task 2 focused checks, and the `hygiene` wiring (line 186) still names only
  `check-docs.py` and `test-check-docs.py`, neither of which invokes `git`. The shallow-clone
  failure mode is gone, and the one-time check no longer runs on every gate. This is the second
  of the two rewrites I offered; it is acceptable because the migration is a one-time event and
  its evidence is recorded.
- **A8 — DISCHARGED.** Line 146: "`check-docs.py` counts the hand-written `### <tool>` headings
  outside the generated block, against the tool names inside it (the block itself is byte-pinned
  by the Rust test). There must be exactly one heading per name, and no headings for unknown
  names." That is the check I asked for, and the redundant check of the generated block is gone.
- **A9 — DISCHARGED.** Line 147: every `examples/*.json` must be "either a listed tool request or
  a listed supporting input (`temperature.schema.json` and `temperature.input.txt`, consumed by the
  temperature request)", with no orphans. The schema file now has an explicit, listed role rather
  than a hand exemption.

No new design text was introduced by these edits beyond the three sentences above. The remaining
design is one checker script with one test, facts and lists taken from `RELEASE_ORDER`,
`fuzz.TARGETS`, `RECEIPT_VERSION`, the dialect doc stems (pinned to `BUILT_IN` by a Rust test),
`docs/known-limitations.json`, and a registry-generated `TOOLS.md` block byte-pinned by
`protocol.rs`; every rule fails loudly on absence and has a failing fixture; the social-preview
pair is pinned by output digest with its tool and font recorded.

VERDICT: PASS
