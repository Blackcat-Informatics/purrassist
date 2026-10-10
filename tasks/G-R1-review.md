# G-R1 review: repin Node to v26.11.1 (G1)

## Scope
Staged/uncommitted: vendor/tooling.json node entry; rename vendor/licenses/node-26.10.0 -> node-26.11.1 (LICENSE, 0 byte change). No installs, read-only.

## Evidence
1. All five pin fields updated (version, source, notices key, binary_source, binary_sha256); JSON parses. Notices hash unchanged by design.
2. SHASUMS256.txt (nodejs.org/dist/v26.11.1) for node-v26.11.1-linux-x64.tar.xz = 3883bfc7...5b651, equals binary_sha256.
3. Upstream v26.11.1 LICENSE sha256 = 6b85ee19...fc4128 = notices hash = sha256 of vendored file.
4. Remaining "26.10.0": only docs/qualification/fuzz-{,smoke-,stage2-}pattern-linux-x86_64.json ("node": "v26.10.0", historical run records) and .stage notes. No script reads these records back against the pin (grep of scripts/.github/Makefile found no consumer), so keeping them is correct: they record the toolchain actually used for those runs; rewriting would falsify them.
5. Consumers (check-wasm.py, check-syntax-oracles.py, fuzz.py) read the version from tooling.json by name; install-qualification-tool.py builds the path from entry['version']; measure-resources.py/check-release.py/check-repository.py read the inventory generically. No hard-coded version in scripts or workflows (workflows have no node-version references).
6. `python3 scripts/check-repository.py` exit 0 (inventory/notices verified); `python3 scripts/test-fuzz.py` 66 tests OK.
Orchestrator evidence: make wasm-test exit 0 under local Node v26.11.1.

## Findings
None blocking. Note: checksum is from the official SHASUMS256.txt over HTTPS, not GPG-verified (as the plan states).

VERDICT: PASS
