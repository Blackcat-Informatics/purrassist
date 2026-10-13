# Validation index — issue #1, PR #25 (post-publication record)

- Publication evidence: release qualification run 38356797286 passed; publish run 38356797476 passed (verify-published OK); crates.io shows 0.1.0 for all six crates; the GitHub release rust-v0.1.0 exists; docs.rs status.json reports doc_status true for the five library crates.
- Local download check: cargo install purrassist-cli 0.1.0 into a clean root; decode, call match_strings, call apply_edit (exact 'hello earth\r\n' with an inverse), MCP initialize/tools-list (31)/tools-call all OK.
- server.json validated with `purrassist validate` against the MCP Registry 2025-12-11 schema (valid true, after shortening the description below 100 characters).
- check-docs.py OK; test-check-docs.py 252 OK; make hygiene exit 0 on f58736a.
- Merge-plan review PASS. Its five live checks were run by the coordinator: all match. Its A1/A2 wording was fixed in e19c8d8; make hygiene exit 0.
