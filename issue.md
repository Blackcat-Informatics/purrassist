# Issue #266: Govern RDF loader role and ownership classification

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github
Labels: enhancement

## Body

Part of #33.

The RDF loader classifies explicit, linked and inferred roles and indexes the rows owned by FnO and SHACL. Those tables currently allocate outside the caller's scratch account and clone retained RDF terms.

Use admitted storage and the native term identities already available from the retained parse. Keep explicit and linked roles ahead of inferred roles, preserve blank-node scope, and preserve deterministic diagnostic order. Keep list ownership and cycle termination. Resource or cancellation refusal must release the table's temporary storage and withhold the graph.

Acceptance

- The production loader uses the governed role and ownership tables through classification, row validation, FnO catalog construction and typed tool building.
- Existing loader and manifest contracts hold. A focused regression exercises refusal in the classification stage and verifies its scratch account is refunded.
- The local gate passes.

Published-dataset ownership, native SHACL admission and WASM integration remain part of #33.


## Comments (0)

