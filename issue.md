# Issue #272: Govern RDF vocabulary and statement ownership scans

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

Part of #33. Apply the caller's RDF vocabulary to the full statement value, including literal datatype IRIs, embedded triple terms, and reifier identities. Preserve the first written source extent and the refusal order for existing top-level cases.

Use admitted iterative traversal and charge namespace and vocabulary comparisons through the caller's governor. Apply the same namespace comparison to statement ownership and the temporary FnO view. Keep named-graph refusals and the native SHACL acceptance under #33.

Qualify an unknown IRI in each newly covered value kind, neighbouring valid input, and a refusal reached during vocabulary scanning under a limited budget.


## Comments (0)

