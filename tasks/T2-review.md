# Task 2 independent review

## Scope
Uncommitted diff against HEAD dbdc64d in the worktree: README.md, docs/TOOLS.md (new, intent-to-add), docs/IMPLEMENTATION.md, six crate READMEs, crate Cargo.toml metadata (cli, host, facade), crates/cli/tests/protocol.rs, scripts/check-docs.py, check-repository.py and their tests. Checked against plan.md "Task 2" plus every contract row naming Task 2, output-rules.md and validation.md "Task 2".

## Evidence (commands run, all exit 0)
- `python3 scripts/check-docs.py && test-check-docs.py (137 tests) && check-repository.py && test-repository-audit.py (42) && check-release.py && test-check-release.py (18)`: pass.
- `cargo test -p purrassist --doc`: 5 passed. `cargo test -p purrassist-cli --test protocol`: 8 passed (includes tools_reference_matches_registry, examples_run_through_cli_and_mcp, validate_command_prints_validate_json_output). `cargo fmt --all --check`: clean. `cargo clippy -p purrassist-cli --tests -- -D warnings`: clean.
- Makefile runs check-docs, test-check-docs, check-repository, test-repository-audit (Makefile:12-17) and `cargo test --workspace` (Makefile:26), so all new rules and Rust tests are in `make check`.

## Mutation probes (all reverted, sha256 of TOOLS.md and cli README verified identical, `git status` unchanged, examples/ restored)
1. Edited `match_embeddings` description in the generated block ("Match" to "Matches"): `tools_reference_matches_registry` FAILED with a line diff (docs/TOOLS.md line 5 vs registry).
2. Deleted `### \`read_file\``: check-docs reported `[tools-headings] ... tool 'read_file' needs exactly one heading ... found 0`.
3. Wrapped the mcp-name bullet in an HTML comment: `[registry-name] crates/cli/README.md: needs a visible "## Links" bullet ...`.
4. Added `examples/orphan.json`: `examples_run_through_cli_and_mcp` FAILED ("every examples/ file must be listed in EXAMPLES ...").
5. `CI=1 PURRASSIST_BLESS=1`: test panics "must never run with CI set" before writing.

## Deliverables and contract rows
- Three-interfaces block: README.md:25-101, Rust / MCP / CLI subsections as equal `###` headings, in checked order; Rust snippet is a byte copy of the doctested block (crates/purrassist/README.md `three-interfaces-rust`), enforced by generalized `check_marker_copies` (check-repository.py:278-321) covering `derive-example` and `three-interfaces-rust`, with failing fixtures (test-repository-audit.py:204-270). MCP config JSON parsed by `check_mcp_config` (command, args shape; fixtures test-check-docs.py:778-800). `.purrassist/` sentence README.md:84-86; CLI=MCP sentence cites the test name (README.md:92-95; existence verified by check-docs). decode/match described as shortcuts with own output; validate claim backed by the new test.
- Crate map equals RELEASE_ORDER (rule `crate-map`, README.md:105-118; fixtures 701-726); internal crates named.
- docs/TOOLS.md: generated block between markers pinned byte-for-byte by the Rust test with BLESS semantics and CI guard (protocol.rs:1306-1352); each entry shows MCP form, CLI form and example links; one `###` heading per tool in registry order including `read_file` (TOOLS.md:813) and `undo_file` (841); `tools-headings` rule counts headings outside the block and rejects unknown names (fixtures 663-693).
- `examples_run_through_cli_and_mcp` (protocol.rs:1002): EXAMPLES table, orphan and stale-listing both fail, supporting inputs listed, temperature checked via decode_json in both transports, output-schema validated.
- `validate_command_prints_validate_json_output` (protocol.rs:1073): valid (exit 0) and invalid (exit 4) instances, stdout byte-equal to `call validate_json`.
- mcp-name visible bullet (crates/cli/README.md:104) derived from workspace repository owner, case-sensitive, outside comments/fences; fixtures for missing, commented, wrong owner (test-check-docs.py:885-915).
- Cli documentation not docs.rs: Cargo.toml `documentation` = repo docs/TOOLS.md; rule `cli-documentation` with fixtures.
- Packaged READMEs derived from `cargo metadata` (publishable packages equal RELEASE_ORDER), banned phrases and `docs.rs/.../latest` checked; derive README pinned to 0.1.0.
- IMPLEMENTATION workspace boundaries equal workspace members, unpublished marked internal (IMPLEMENTATION.md:96-103).
- Metadata matches the (revised) plan row: cli description/categories/keywords/documentation, facade description, `mcp` keyword on host and cli; no `homepage` anywhere and no `redundant_homepage` allow (grep of Cargo.toml files empty; root Cargo.toml unmodified).

## Real callers / consumers
`Tools::definitions()` is the registry consumed by the CLI `call` (crates/cli/src/commands.rs:380-392), MCP (mcp_stdio.rs) and the new generator; `Store::open` creates `.purrassist` for both `mcp` and `call` (files.rs:319-340), so the README/TOOLS journal claim is true even for read-free tools. check-docs `check()` wires every new rule (check-docs.py ~920-955) and is invoked by `make check`.

## Claim accuracy (verified against code)
- `.purrassist/` journal: true (files.rs:324, tools.rs:46; `call`/`mcp` always call Tools::new).
- `read_file` narrative: 16 MiB (tools.rs:46), path rules (files.rs:678-690 resolve: relative, no ./.., empty, backslash, colon, NUL, `.purrassist`, symlink, regular file), UTF-8 rejection, no writer lock (read at files.rs:347 takes none), digest form, use as `expected_digest` (tools/files.rs:50-54): all correct.
- `undo_file` narrative: transaction identity from apply_file/commit_files, inverse applied to current text, Stale on change, journaled as new transaction, commit-shaped result (files.rs:490-512; COMMIT_OUTPUT schema): correct.
- decode / match / validate: exit codes 0/3/4, `--input` or stdin, `--effort`, `--no-guesses`, match exit 4 when empty, position ids (commands.rs:90-170): correct. `validate` emits the tool output via `validate_json` (commands.rs:90-108): correct and tested.
- Install wording: root README makes `cargo add`/`cargo install` conditional on publication and points at the checkout; crate READMEs only appear on crates.io after publication. Acceptable.
- No new sentence contradicts code found.

## Migration check
Re-ran the validation.md script verbatim: 248 tokens, 14 link targets, 2 tokens absent, 0 links absent, identical to the recorded output. Also ran an independent sentence-level diff of old vs new README+TOOLS narrative: every unmatched sentence is either reworded in TOOLS.md (helper pair, witness tool, curated quality set, markdown_context, apply_edit, effort args, inspect_code), deliberately replaced by Task 1 (Pre-release / review-and-merge wording, `make fuzz TARGET=json`), or the respelled `call TOOL --root DIR`. The two accepted removals are acceptable: the fuzz sentence was already superseded in dbdc64d and `TARGET=json` survives as an example; `call TOOL --root DIRECTORY` is respelled with the same claim preserved.

## Output rules
No attribution, agent names (other than the product's own Anthropic/Claude-format dialect vocabulary, which is product content), or hosted-CI account/payment wording in any changed file. The only "CI" mention is the BLESS guard.

## Findings (none blocking)
1. README.md:17-24 status paragraph is dense with commit hash and PR numbers (`PR #2, 699d9ff`, #3, #8, #15); accurate and Task 1 text, but heavy for a front page. Suggest trimming in Task 3 when the CHANGELOG exists. No plan/task/stage jargon found.
2. README.md:3-10 does not mention the three interfaces before the status paragraph; they start at line 25 under a prominent `## One engine, three interfaces`. Acceptable; optionally add one sentence to the opening.
3. check-docs.py `check_packaged_readmes` regex `docs\.rs/[^/\s)>"]+/latest` would miss `docs.rs/crate/NAME/latest`; no such link exists. Cosmetic.
4. The migration check uses substring containment (`t not in new_text`), so short tokens could pass vacuously. Supplemented here by the sentence-level diff, which found nothing lost. Cosmetic.

VERDICT: PASS
