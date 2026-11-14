# Independent review: issue 205 property test, head d5a297a

Reviewer: independent read-only review (delivery-reviewer, Opus), diff 1eef154..d5a297a (one new test file).

VERDICT: PASS at head d5a297a. No blocking findings.

Optional findings, recorded as known limits and not fixed in this change:
1. The module doc says the generator varies projections; the projection is always the same shape and only the tool index changes its value.
2. Annotation presence is not varied: the four hints are always emitted, and their values are correlated (destructive is the inverse of read-only; idempotent equals read-only; open-world equals egress). No graph without annotations is generated.
3. The span test reads the generator's own labels rather than the loaded graph, so a Turtle error that drops a hint would not fail it. Zero-parameter tools are never generated.
4. The ledger soundness check is partly redundant with encode's internal check; it still performs a real lookup over the generated set.
5. Byte identity by re-encoding in one process catches run-varying order, not a fixed but different key order; the golden test in mcp_manifest.rs covers fixed order.
6. Each of the four tests regenerates and reloads all eight graphs (cost only).

Answers: variation is limited by loader constraints (one declared output, a required outputProjectionJq), which the test states only in inline comments. Assertions check the stated properties. The ledger check is not a tautology. Determinism: fixed-seed xorshift, BTreeSet and Vec only. No dependency added. SPDX header present.
