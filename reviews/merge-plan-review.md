# Merge-plan review: PR #25 (issue #1), head f58736a, base main 598d059

Reviewed plan: `/home/paudley/Active/purrassist/.worktrees/1-complete-purrassist-0-1-0-plan-and.squash_notes`

Supporting material read: `validation.md`, `issue.md` (the issue body, the Stage 1 plan rows on `mcp-name` and
`server.json`, the Task R acceptance, and the last four issue comments), `pr-checks.txt`, `pr-comments.md`,
`AGENTS.md`, and the changed files at HEAD: `README.md`, `SECURITY.md`, `docs/IMPLEMENTATION.md`,
`docs/QUALIFICATION.md`, `docs/RELEASE.md`, `server.json` and `scripts/test-check-docs.py`. I also read the
unchanged `scripts/check-docs.py` (`check_workspace_version`), `crates/cli/{Cargo.toml,README.md,src/commands.rs}`,
`scripts/data/verify-published/mcp-responses.jsonl` and the git refs under `/home/paudley/Active/purrassist/.git`.

## Limits of this review

This session had no shell or network tool, so I could not run `git diff`, `gh`, curl or the docs.rs request, and
I could not fetch the MCP Registry package-types page. Instead I compared the changed files at HEAD against the
squash notes' file list. Five live claims rest on `validation.md` and were **not re-observed** by me:

- the crates.io `max_version` of the six crates;
- `gh release view rust-v0.1.0`;
- the publish run 38356797476 conclusion;
- the docs.rs `status.json`;
- the GitHub homepage URL.

Before merging, the caller should run the five read-only commands it listed. Any mismatch reverses this verdict.

## Coverage

| Check | Result |
|---|---|
| The tag names `598d059f956aa46315b6a4890577684e536c04c8` | Consistent. `refs/heads/main`, `refs/remotes/origin/main` and `refs/ghprsq/pr/24/result` all hold that full SHA, and git decorates `598d059` with `tag: rust-v0.1.0`, so the annotated tag peels to it. The earlier tag on `464306c` matches `refs/ghprsq/pr/23/result` and the 05:43 issue comment. |
| "31 tools" in `tools/list` | True. The committed `mcp-responses.jsonl` lists 31 tool names, and `docs/TOOLS.md` has 31 tool headings. |
| "five library crates" on docs.rs | Consistent with the tree. `src/lib.rs` exists for core, derive, syntax, purrassist and host. The cli is bin-only, and wasm-harness is unpublished. |
| Five status docs flipped | All five in `STATUS_DOCS` read "published as `rust-v0.1.0`": README (capitalised at the start of a sentence, which the checker accepts case-insensitively), SECURITY, IMPLEMENTATION, QUALIFICATION and RELEASE. No stale "not yet tagged", "unreleased" or "no publication" wording remains in tracked Markdown. |
| `server.json` matches the CLI usage | Yes. `commands.rs:394` requires exactly `mcp --root DIR` as separate arguments, which matches the positional `mcp` plus the named `--root` (`isRequired`, `format: filepath`). `crates/cli/README.md:103` carries the visible `mcp-name: io.github.Blackcat-Informatics/purrassist` line. The file is unchanged by this PR, so it was present in the published 0.1.0 package, which Cargo auto-includes as `README.md`. The description is under 100 characters. The top-level version and the package version are both `0.1.0`. |
| `server.json` against the cargo package-types page | Not re-fetched (see the limits above). Stage 1 recorded `cargo` support and `mcp-name` README ownership as verified against that page (issue.md lines 2564 and 2802). The notes correctly keep submission, the only real proof, as an owner step. |
| The fixture change does not weaken the rule | Confirmed. `scripts/check-docs.py` is not in the diff, so the rule itself is unchanged. These fixtures still fail as they should: the mix test (SECURITY only, README+RELEASE, all but RELEASE) fails with `mix forms`; a wrong version fails under both forms (`0.0.9`, `0.1.1`, a `0.2.0` bump); a line in neither form fails with NOT_FOUND; "Not yet tagged" alongside four "published as" lines fails. `Tree.sub` raises when nothing matches, so the `EITHER` rewrite cannot pass as a no-op. |
| Closing #1 is justified | Yes. Every "Release acceptance" bullet in the issue body is now discharged: the suites and gates, the archive and notice audit, verification of downloaded packages, hosted CI, trusted publishing, publication and the release. Task R's acceptance (publish-run success, 0.1.0 served, `verify-published`, the GitHub release) is recorded. `server.json` submission was declined from scope in Stage 1 as a post-publication owner step (issue.md:2585-2588). The social preview was made an explicit owner decision (issue.md:2570). Neither belongs to the issue's acceptance. |
| Honesty and hygiene of the notes | Clean. There is no agent credit, co-author trailer, ghprsq trailer, or wording about hosted-CI accounts or payments. The failed first tag is disclosed together with the reason no publish ran. `server.json` validation is described accurately as a schema check, not a registry acceptance. The PR has no comments. |

## Findings

No BLOCKING findings.

**A1 (ADVISORY): the procedure text contradicts the record.** `docs/RELEASE.md` step 4 says:

> "Tag `rust-vVERSION`. Tagging is irreversible and requires explicit authorization"

The new record in the same file says:

> "The tag was moved to the fixed commit, and nothing had been published from it."

The step overstates what is irreversible: publishing is irreversible, and moving an unpublished tag is not.

REWRITE TO: "Tag `rust-vVERSION`. This requires explicit authorization. Publishing cannot be undone. A tag may be
moved only if its release qualification failed and no `cargo publish` ran from it." This is optional for this PR.

**A2 (ADVISORY): the README wording mixes a git tag with crates.io.** `README.md:15` says:

> "Published as `rust-v0.1.0` on crates.io"

`rust-v0.1.0` is a git tag. crates.io holds version `0.1.0`.

REWRITE TO: "Published as `rust-v0.1.0` (version 0.1.0 on crates.io)". This keeps the checked status form.

**A3 (ADVISORY): binary name versus cargo identifier.** The registry entry names the package `purrassist-cli`, but
the executable is `purrassist` (`crates/cli/Cargo.toml` `[[bin]] name`). If the registry's cargo launch convention
assumes the binary is named after the package, clients would fail to start the server. Confirm this with
`mcp-publisher` and the package-types page at submission. This is already an owner step, so nothing changes in
this PR.

**A4 (ADVISORY): merge timing.** `pr-checks.txt` shows all eight hosted checks `IN_PROGRESS`. The merge should
wait for them to pass, which the ghprsq workflow enforces. `server.json` has no SPDX header, but that follows the
repository's precedent for JSON (`examples/*.json`, `docs/assets.json`): `check-repository.py` checks headers only
on Markdown, and `make hygiene` passed.

VERDICT: PASS
