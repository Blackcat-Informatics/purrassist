# Issue #183: Main's tool-call fuzz target fails: a rejected attempt runs past its failure point

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

The tool-call fuzz target fails on main (dade01d) at execution 710 with an assertion at `crates/core/examples/fuzz_tool_call.rs:1264` about a rejected glm-4.5 attempt running past its failure point.

Reproduction: `python3 scripts/fuzz.py --target tool-call --timeout 120` (fails at execution 710). The failing input is saved in the repository's review record as a binary file and reproduces identically on dade01d.

Earlier, this same target passed on the repair stack (#175 PRs 176-179). A merged change since then lets a rejected attempt run past the point where it fails. The fix must find that change and fix the code, not the oracle: the saved input becomes a pinned regression case, and the oracle assertion stays as it is.

Acceptance: the saved input is a pinned regression test that fails on dade01d and passes after the fix; the fuzz target passes with `--timeout 120`; `clippy --workspace --all-targets -- -D warnings` exits 0.


## Comments (0)

