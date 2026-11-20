# Issue #39: Tool graph phase 4: verification and conformance suite

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github
Labels: enhancement

## Body

## Objective

Run the conformance and verification suite that qualifies the tool graph as a whole. Phase 3 sub-issues carry their own focused tests. This phase adds the cross-cutting goldens and properties and closes the qualification record.

## Depends on

- #33, #34, #35, #36 and #37: all five complete implementation contracts delivered.
- #40 coordinates this checklist after consolidation of the redundant wrapper #38. No implementation or qualification acceptance was removed.

## Suite

- **Turtle 1.2 ↔ MCP Extended JSON round trip.** The TOOLS.md worked example and a generated corpus over the vocabulary must satisfy `decode(encode(g))` ≅ `g`, modulo exactly the ledgered losses, checked with `purrdf_core` dataset isomorphism.
- **LossLedger completeness and soundness.** Every lane's ledger is checked for every family under the registered loss profile. A mutation test removes one emitted constraint from a lane and confirms the ledger check catches the difference.
- **GBNF.** Goldens for each family. The grammar is accepted by the llama.cpp grammar parser, or by the recorded equivalent. A property test checks that every accepted value validates or fails only on a ledgered constraint.
- **AppArmor synthesis.** Goldens pass `apparmor_parser -QK` 4.1.x. **Negative Datalog conflict tests** check that a `readOnlyHint` tool declaring `w`, `a`, `k` or `l`, or `mount`, and an `egressAllowed false` tool declaring `network inet`, are each refused with the SARIF golden. The `/8` network refusal is included.
- **Determinism.** Byte-identical artifacts and receipts across two runs, and across reordered input triples.
- **Native worked-example artifacts.** Compile every lane for the worked example and match the pinned native artifact bytes.
- **Qualification record.** `docs/QUALIFICATION.md` gains a tool-graph section that records exactly what ran, including the AppArmor-enabled confinement smoke test and the host it ran on.

## Acceptance

- `make check` passes. It is the repository's gate and includes `cargo fmt --check`, workspace clippy on all targets with `-D warnings`, and the tests.
- Every new test names the production behavior or invariant it protects.


## Output unit acceptance (#47)

Qualify #47 through full v2 manifest round trips preserving byte units and exact enforcement, rejection of v1/legacy token fields and invalid modes, and actual result-release boundary tests. Include exact-bound success, one-byte-over refusal, a multibyte result whose deterministic token count fits while its bytes exceed the bound, explicit governance failure, and receipts recording unit/mode/limit/measured. Qualification evidence must distinguish these result-byte semantics from prompt token accounting.


## Comments (1)

### paudley — 2026-11-19T11:33:44Z

The worked MCP manifest is sensitive to the order of equivalent Turtle
statements. This reproduces on `34d1f9da` with PurRDF 4.0.2.

In the worked example, exchange the complete
`fno:expects ( ex:customerParam ex:limitParam )` and
`fno:returns ( ex:ordersOutput )` clauses. Keep each list's member order.
A focused native probe ran both inputs through `ToolGraph::load` and
`mcp::encode`. PurRDF's direct comparator found the datasets isomorphic.
The typed tools and loss ledgers matched. The serialized Turtle and
complete manifests differed.

The parser assigns anonymous list cells labels in encounter order. The
serializer sorts the resulting labels. PurRDF's canonical relabeler
has a finite search budget and a stop signal, but its dataset rebuild
has no caller allocation admission. The loader also admits blank hint
reifiers, and their identities appear in both the retained graph and
the projected `recoveryHints`. Any canonicalization must preserve that
correspondence through decoding.

PR #258 checks structural round-trip identity. Reordered-input byte
determinism remains open.


