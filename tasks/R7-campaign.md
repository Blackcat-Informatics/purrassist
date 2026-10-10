# R7 campaign results (G4 fuzz reach, G9 trailer claim)

Head a49e81b plus the uncommitted R7 change (Makefile, fuzz_tool_call.rs, docs/FUZZING.md, scripts/fuzz.py, scripts/test-fuzz.py). Not committed.

## What changed

- `crates/core/examples/fuzz_tool_call.rs`: `--tools fixed|ref` (default fixed; any other argument is a usage panic). `--tools ref` compiles `REF_TOOLS` with `ToolSet::from_openai`: `read_file` (so the forced header still names a tool), `batch_refs` (outer `name` + array of `$ref` items whose `name` collides + string array), `any_items` (`anyOf` object|string items), `tree_walk` (recursive `$ref` tree), `free_items` (`additionalProperties:true` items), `list_records` (inline `array(object)` items, one member an array of strings), `root_ref` (Draft-07 `$schema`, root `$ref` into `definitions`, array-of-object items in the target).
- Deviation (stated): the read-only `Param::array(Param::object(..))` tool was NOT added to the existing fixed set. A new name there can change fuzzy name matching / ambiguity for existing seeds and nothing in the probe prints readings, so "does not change meaning" cannot be shown. `list_records` lives in the `ref` set (all OpenAI tools are mutating by adapter default; no read-only tool is possible there). `tool-call` seeds and tool set are byte-for-byte unchanged (no R4 seed-4 change was needed by R7).
- `from_mcp` mixing is not possible (one adapter builds one `ToolSet`), so the root-`$ref` tool is the OpenAI form with `definitions` and a Draft-07 `$schema`.
- `scripts/fuzz.py`: target `tool-call-ref` (`example: tool-call`, `probe_args: ['--tools','ref']`, 101 seeds = 48 turns (incl. negatives) x 2 headers + 5 strict, own tokens). Helpers `example_name`, `probe_arguments`; build/harness identity use the example name. `--continue-on-known` with `KNOWN_FAILURES` (the two #6 signatures) and `known_failure()`; matching inputs are retained under `known-6/<sha>.bin|.stderr`, listed in `report.known_failures`, subtracted from `crashes`; any other failure still stops. Makefile `make fuzz` loop gains `tool-call-ref`, never passes the option (a test asserts it).
- Seeds cover: top level, in-object (tree), mixed JSON/nested (both orders), unclosed last, unclosed before sibling (closed/unclosed), same-name member, string items, `$ref` items (+ nested-unclosed), colliding names (both orders, unclosed), recursive (balanced, reordered, unclosed), anyOf, opaque (before/after sibling), root-`$ref` call (two orders), the shapes as Hermes/Mistral JSON (+ truncated), markers/markup in values, think/prose, invalid UTF-8, truncated item, negatives. Strict seeds: the 5 in `TOOL_CALL_REF_STRICT`, only seeds with the forced header.
- `scripts/test-fuzz.py`: 78 tests. New: G9 `test_forced_header_seeds_with_a_trailer_byte_are_registered_strict_seeds` (both targets), `..._unchanged_by_the_ref_target`, `..._registered_with_the_ref_tool_set` (target, Makefile loop, Makefile never has continue-on-known, docs mention), `..._cover_every_nested_shape`, `..._run_ok_in_the_built_probe_under_both_tool_sets` (+ usage panic), `test_probe_runs_the_selected_tool_set`, `test_known_failure_signatures_are_only_the_issue_6_panics`, and three campaign tests (continue retains and continues; without the flag the first #6 panic stops; any other failure still stops).
- `docs/FUZZING.md`: ten targets, `tool-call-ref` bullet, "Isolating issue #6" subsection (option, attribution procedure, coverage procedure).

## Checks

| Check | Result |
|---|---|
| `python3 scripts/test-fuzz.py` | 78 tests OK |
| new seeds piped to built `fuzz-tool-call` | `--tools ref`: 101/101 `ok`; no argument (fixed set): ref seeds 101/101 `ok`; fixed `tool-call` seeds 408/408 `ok` (with and without `--tools fixed`) |
| `make hygiene` | exit 0 |
| `python3 scripts/test-verify-published.py` | 10 tests OK |
| `cargo test -q -p purrassist-core` (full) | exit 0, 69 result lines, 1218 passed, 0 failed |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo fmt --check` | exit 0 |
| stray `*.profraw` | deleted (4+1 in the worktree root, from probe replays); none outside `.cache` campaign dirs |

## Campaigns (default 1,000 mutations, default seed `0x5A1727EE614D934B`, `--continue-on-known`)

| Target | Campaign dir (.cache) | Executions | Seeds + mutations | Retained | Regions | Known (#6) retained | Branch-only failures | Status |
|---|---|---:|---|---:|---:|---:|---:|---|
| tool-call | fuzz-tool-call-w6hytlr5 | 1408 | 408 + 1000 | 477 (474 distinct files) | 21,496 | 4 | 0 | passed |
| tool-call-ref | fuzz-tool-call-ref-ihimjlwo | 1101 | 101 + 1000 | 253 | 21,318 | 2 | 0 | passed (`--timeout 120`, see below) |

The first `tool-call-ref` attempt at the default `--timeout 10` (fuzz-tool-call-ref-1fe9v7fu) failed at execution 682 with a probe timeout, not a panic: see "Slow input".

### Attribution (base 943fa04 probe, temp worktree /tmp/r7-base, removed)

Base probe located with `cargo metadata` run inside the worktree (target dir /opt/.cargo/slots/d082d746402abb88/0/target). Each retained #6 input replayed on it, every one exit 101 with the identical F4 panic ("rejected attempt A..B runs past where it failed (N) without its closer: ..."; branch line 1156, base line numbered differently, same message, same span numbers):

tool-call (4 of 4 reproduce on base):
- 4c0954c3c7fa (192 B): `rejected attempt 1..190 runs past where it failed (39)`
- 8097dc89cbb9 (106 B): `rejected attempt 4..65 runs past where it failed (35)`
- 833c3935b350 (511 B): `rejected attempt 184..394 runs past where it failed (212)`
- ff4c72662cfd (410 B): `rejected attempt 0..394 runs past where it failed (52)`

tool-call-ref (2 of 2 reproduce on base; the base has no `--tools`, so the same bytes ran against its fixed set, and also against the branch's fixed set and ref set; identical panic in all three, the F4 oracle does not depend on the tool set):
- 2bfd4b4dc6ab (268 B): `rejected attempt 41..101 runs past where it failed (77)`: `<parame{ter=records>...`
- e806e7b8c60a (197 B): `rejected attempt 0..124 runs past where it failed (36)`: `<parametert_records>...`

Counts: retained known = 6; reproduced on base = 6; branch-only = 0. Both signatures present in the code, only "runs past where it failed" fired (the "beyond a literal past its end" signature has a unit test only).

Also replayed the entire retained corpora on the base fixed-set probe: tool-call 474/474 `ok`, tool-call-ref 253/253 `ok` (no divergence; strict-trailer bytes are just turn bytes on the base).

### Slow input (reported, not a #6 and not a panic)

`.cache/fuzz-tool-call-ref-1fe9v7fu/current.bin` (196 B; free_items with `</``pe"e` mutations) exceeds the 10 s per-probe timeout under the campaign build (opt-level 1, coverage, debug assertions) but completes `ok`. Wall time, same bytes:

| Probe | Time |
|---|---:|
| base 943fa04, fixed set, campaign flags minus coverage | 10.4 s |
| branch, fixed set, same flags | 9.4 s |
| branch, `--tools ref`, same flags | 7.2 s |
| base release / branch release (fixed) / branch release (ref) | 5.4 / 5.7 / 7.2 s |
| debug: base / branch fixed / branch ref | 7.0 / 12.4 / 12.3 s |

The base is as slow, so this is a pre-existing cost of the repair search (perf: dominated by `Vec<Evidence>::clone` and digest compress in the search states), not caused by this branch. Consequence for the gate: `make fuzz` runs `tool-call-ref` with the default 10 s timeout and the fixed seed, which reaches this input at execution 682 on this machine and would fail there before it reaches its two #6 inputs; `tool-call` already fails there on #6. I did not change the default timeout or hide it; the campaign above used `--timeout 120` (a documented existing option). Orchestrator decision needed on whether to raise the default or fix the search cost (not R7 scope).

## Coverage evidence (llvm-cov, campaign `probe` executables, full execution streams)

Method: rebuilt each campaign's exact stream from `report.json` (fuzz.py's `Random(seed)` + `mutate`, corpus = seeds + `retained`; every retained digest and every known digest matched), ran each of the 1,408 / 1,101 inputs through the campaign's instrumented `probe` with its own `LLVM_PROFILE_FILE`, `llvm-profdata merge`, `llvm-cov export probe -instr-profile=...  xml.rs scope.rs`, and counted an execution as reaching a group when a code region (kind 0) with count > 0 lies inside the group's lines. The 6 known (panicking) executions yield no profile and are excluded (listed as `mutated:known`). Script was a temp file (/tmp/r7cov.py), not committed.

Line numbers are the head's (the plan's ~857-865 and ~1660-1663 have drifted: pop's `if frame.item` is now xml.rs:882-889, `nesting` is 976-1006).

`tool-call-ref` (101 seeds + 1000 mutations; 998 mutated executions profiled, 2 known):

| Group | Lines | Seed executions | Mutated executions |
|---|---|---:|---:|
| `pop` item branch (ArrayItem record, `wrap_item`) | xml.rs:883-889 | 64 | 214 |
| `nesting` graph-node closure | xml.rs:981-985 | 28 | 136 |
| `nesting` unmodeled(item): `ArrayOfObject` arm | xml.rs:988 | 28 | 136 |
| `nesting` declared `Array(Object)` item arm | xml.rs:997 | 43 | 216 |
| `nesting` declared `Array(Unmodeled)` item arm | xml.rs:998 | 19 | 79 |
| `nesting` undeclared member `ArrayOfObject` arm | xml.rs:1004 | 8 | 52 |
| `Scope::of_node` | xml.rs:1856-1862 | 28 | 136 |
| `Scope::declares` Schema arm | xml.rs:1868 | 28 | 121 |
| `Scope::member` Schema arm | xml.rs:1899-1905 | 28 | 121 |
| `ScopeNode::member` | scope.rs:129-134 | 28 | 121 |
| `ScopeNode::declares` | scope.rs:138-140 | 28 | 121 |
| `ScopeNode::is_opaque` | scope.rs:154-156 | 28 | 136 |
| `ScopeGraph::node` | scope.rs:176-178 | 28 | 136 |

`tool-call` (408 seeds + 1000 mutations; 996 mutated profiled, 4 known): all 13 groups have 0 seed and 0 mutated executions. The fixed tool set has no array of objects and no unmodeled parameter, so it never reached these branches; that is G4 and the reason `tool-call-ref` exists. (Scope-graph compile paths in `scope.rs` such as `ScopeCompiler` run only when `from_openai` builds the ref set, at probe start; not counted as mutated-execution evidence.)

## Result

- G4: closed. Mutated executions reach the item branches of `pop`/`nesting` (214 / up to 216) and the runtime scope-graph paths (121-136) under `tool-call-ref`; none under `tool-call` before.
- G9: closed by the new test (both targets).
- #6 isolation: 6 known inputs retained, 6 reproduce on base, 0 branch-only; no new failure class. Isolation was feasible, so no seeds-only fallback.
- Open for orchestrator: the slow-input timeout (pre-existing, 10.4 s on base) vs `make fuzz`'s default 10 s timeout; and `make fuzz` stays red on #6 for `tool-call` and (via the same panic, later in the stream) `tool-call-ref`.
