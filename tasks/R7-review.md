# R7 review (G4 fuzz reach, G9 trailer claim)

## Scope
Uncommitted diff vs a49e81b: fuzz_tool_call.rs, scripts/fuzz.py, scripts/test-fuzz.py, Makefile, docs/FUZZING.md, docs/RESOURCES.md; plan "### R7", R7-campaign.md, issue #11.

## Evidence
1. Existing target unchanged: the fuzz.py diff removes no seed or token line (only the harness/probe plumbing lines and the docstring "nine"); `fixed_tools()` is the old `tools()` body renamed; 408 `tool-call` seeds all print `ok` under the default probe. Only addition: a `strict` key (used by the G9 test) on the target config.
2. `--tools ref` set: REF_TOOLS covers $ref items + colliding names (batch_refs), anyOf items (any_items), recursive $ref (tree_walk), opaque (free_items), inline array(object) (list_records), root $ref (root_ref). Seeds (101 = 48 turns x 2 headers + 5 strict) cover every plan §1 shape; no ref seed uses a forced header except the 5 registered strict ones; headers 0x02/0x00/0x05/0x9c never have bits 6-7 = 11. 10 sampled seeds under `--tools ref`: `ok`. Unknown `--tools` value panics.
3. `--continue-on-known`: only two exact-signature regexes; matching inputs written to known-6/ (.bin and .stderr), listed in report, `crashes` decremented; timeouts (TimeoutExpired) and non-matching CalledProcessError still raise. Tests cover continue, no-flag stop, other-failure stop. Makefile loop has tool-call-ref, a test asserts no continue-on-known in fuzz.py lines.
4. G9: test passes; adding a forced-header seed `\xc6\x23hello\xf9` to both targets via a monkeypatched copy makes it fail (repo not modified).
5. Campaign records match report.json files: tool-call 1408 exec / 477 retained / 21,496 regions / 4 known; tool-call-ref 1101 / 253 / 21,318 / 2 known; known-6 dirs hold 4 and 2 inputs. Coverage method (replay of the deterministic stream through the instrumented probe, region-kind-0 count > 0) is sound; the stream rebuild was digest-checked per the record. Attribution: 6/6 reproduce on base, 0 branch-only, recorded. I did not re-run the campaigns or the base replay.
6. Docs: #11 numbers (192 B, 240-280 ns, 1.65 s maximum, 0.29/0.3 s sized, ~5 s release, execution 682, 10 s timeout) match the issue. 
Gates: test-fuzz.py 78 OK; make hygiene 0; test-verify-published.py OK; full `cargo test -q -p purrassist-core` all pass, 0 failures; clippy -D warnings clean. No stray profraw in the worktree.

## Findings
REQUIRED: none.
ADVISORY:
- A1: KNOWN_FAILURES uses `.*` with DOTALL over the whole stderr; an input whose bytes echo into the panic message could in principle mimic a signature. Anchoring to the "panicked at" line would harden it. Low risk.
- A2: R7-campaign.md says tool-call-ref default `make fuzz` will fail at execution 682 on the timeout (issue #11 accepted by user); the Makefile loop therefore keeps the smoke gate red until #6/#11 are fixed. This is a known, documented limit, not a defect of R7.
- A3: plan asked for a read-only Param::array(Param::object) tool in the fixed set; deviation (put in ref set instead) is justified and documented.

VERDICT: PASS

## Orchestrator disposition
- Advisory 1 (anchor the known-failure regexes): done. Both signatures now require the probe's own `panicked at …fuzz_tool_call.rs:L:C:` line immediately before the message. The `test-fuzz.py` samples use the real format, plus negative cases for an echoed lookalike and for a message with no panic line. `test-fuzz.py` OK, `make hygiene` exit 0. All 6 retained real #6 stderr files still match.
- Advisory 2: accepted limit, issue #11 (user decision). Advisory 3: deviation accepted.
