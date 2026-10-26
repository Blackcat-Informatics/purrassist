## Objective

Resolve the upstream plain FnO argument-schema projection prerequisite discovered while specifying #32 in PR #42. This blocks the plain MCP input/output schemas required by #33; it preserves that parent issue's full loader and bidirectional codec contract.

This issue tracks a prerequisite. The Phase 2 maintainer review stop still applies: no Phase 3 implementation or dependency adoption before the approval recorded on #32. Cross-repository implementation requires its own authorization.

## Verified gap

`purrdf_shapes::json_schema::compile` describes the JSON-LD projection: namespace-compacted property names, class-root definitions, RDF node carriers and typed-literal alternatives. Its `SchemaSurfaceMode` changes ontology/shape coverage, not that representation. The relevant compiler source is unchanged between the specification's recorded `e1ad49f` snapshot and the inspected `2ed9845b0` checkout.

The tool graph needs plain FnO argument objects such as `{"customer_id":"customer-17","limit":10}` and plain output objects. Renaming keys alone cannot establish that contract. A second schema compiler or local JSON-LD schema rewriting is not an acceptable substitute.

## Required upstream contract

Extend the existing upstream compiler with an explicit caller-owned plain argument profile. Proposed names and signatures are in [the G7 plan](https://github.com/Blackcat-Informatics/purrassist/blob/2e9eeb46f29bd223fdfee0b544b4d37429fa024f/docs/tool-graph/TOOLS_PLAN.md#g7-upstream-plain-tool-schema-projection-prerequisite); these remain specification proposals until approval.

- Select both root shape and root class explicitly and validate their agreement.
- Supply complete predicate-to-`fnom:implementationProperty` bindings scoped by parent shape, including nested input and output fields. Refuse missing, duplicate or ambiguous bindings.
- Define plain JSON scalar, object and array representations, requiredness, cardinality and closedness in the existing compiler. Keep exact constraints and references; propagate every existing loss entry with its source.
- Return typed, actionable failures for unsupported RDF representations, invalid bindings, arithmetic overflow and resource exhaustion. No wrapper leakage, silent coercion, partial success or loss of the original SHACL source.
- Preserve the existing JSON-LD compiler's behavior and goldens. Keep the computational path deterministic and buildable for `wasm32-unknown-unknown`.

## Acceptance

- The upstream API is actually merged and available in an identified dependency source; record the upstream issue/PR and exact source revision here.
- A focused downstream consumer compiles and produces the worked tools' exact bare input/output schemas, including `customer_id`, bounded integer `limit`, file arguments and output fields. Exercise nested objects/arrays, required/optional fields and closed objects.
- Invalid root/binding/representation cases and tiny ceilings return the specified typed outcomes. Check exact schema constraints outside the typed core subset, loss propagation and unchanged JSON-LD neighbors.
- Resolve the actual package closure and register/approve any drift under the repository dependency policy before adoption. Core keeps its empty dependency closure.

Production integration remains #33's responsibility: use this API on the actual path, retain exact core schema validation and map arguments back through FnO for original SHACL and policy checks. This prerequisite's focused consumer does not complete that parent behavior. Keep #33 open until its complete original contract is delivered.

Bound implementation units to four hours including qualification and landing; if this prerequisite grows, split complete behaviors while retaining every remaining requirement here.
