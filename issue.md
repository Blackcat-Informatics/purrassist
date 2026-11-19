# Issue #270: Govern typed RDF tool construction and ownership

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

Part of #33. Govern construction of the typed tools returned by the RDF loader. Admit temporary indexes, strings, collections, and the retained tool values before allocation. Keep the retained account with the loaded graph and refund it when the graph is dropped or loading is refused.

Reuse the existing field-mask parser, callable binding, and native storage admission. Preserve the tool data model, deterministic ordering, scoped reifier references, and existing diagnostic precedence. Profile traversal must check the caller's depth and stop limits before descent, with an explicit stack contract for the public recursive values.

Qualify the worked manifest and round trips, nested profiles, and scratch charges on success and refusal. The published RDF dataset, native SHACL validation, and WASM integration remain under #33.


## Comments (0)

