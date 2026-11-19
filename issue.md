# Issue #252: Govern the tool graph loader temporary FnO dataset

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

Part of #33.

The loader's controlled FnO reader consumes a temporary RDF dataset. The
loader currently builds and freezes that dataset outside the caller's
storage account, so the reader can begin with unaccounted live buffers.

Build and freeze the temporary dataset with the pinned PurRDF native
memory APIs. Route allocation, work, and cancellation through the graph
loader's governor. Keep the account alive while the FnO catalog reads
the temporary dataset. Release it after the dataset is destroyed on
both success and refusal. Preserve the existing selected statement set
and typed FnO and resource failures.

Verify that a storage refusal during temporary view construction is
reported by the real loader path. Verify that dropping the view restores
its storage charge, without releasing the independently owned catalog.
Run the graph loader cases and the local gate.

The published dataset's ownership, the remaining temporary row and index
accounting, the original SHACL checks, and WASM integration stay in #33.


## Comments (0)

