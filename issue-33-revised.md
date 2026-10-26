## Objective

Create `purrassist-toolgraph` and its foundation:
- a loader from RDF 1.2 Turtle to a typed, validated tool graph;
- the FnO and SHACL binding;
- a **bidirectional** codec between the tool graph and MCP Extended Tool Manifest JSON, where every projection returns a `purrdf_core::loss::LossLedger`.

Built to the approved `docs/tool-graph/TOOLS.md` and `TOOLS_PLAN.md` (#32).

## Depends on

- #32 approved. That approval includes the dependency closure.
- Blackcat-Informatics/purrdf#747: FnO reader.
- Blackcat-Informatics/purrdf#748: caller-registered loss profiles.
- #43 (G7): upstream plain FnO argument schema profile and qualified downstream consumer.

## Reuse (no second implementation)

- `purrdf_rdf::parse_dataset` / `serialize_dataset`; `purrdf_core` `RdfDataset`, `reifiers_of`, `annotations_of`; `try_canonicalize` (profile `purrdf-rdfc12`) for the graph digest.
- `purrdf_core::fno` (via U1) to read `fno:Function`, `fno:expects` and `fnom:` mappings.
- `purrdf_shapes::shapes::from_dataset` and `validate_dataset_with_governors` to validate the tool graph against the vocabulary's own shapes.
- #43 (G7) supplies an upstream caller-owned plain FnO argument projection profile for each tool's `inputSchema` / `outputSchema`. Current `json_schema::compile` projects JSON-LD and does not satisfy this contract. Use the existing upstream compiler through that new profile, retaining exact constraints and the complete `CompiledSchema.losses` ledger; do not add a second compiler or rewrite JSON-LD schema keys locally. Reverse conversion must preserve the original graph and exactly ledger any losses.
- The caller-owned vocabulary is passed explicitly as a `Namespaces` and prefix table. Library code contains no IRI string literals for it.

## Specification

- **`vocab`:** the namespace table type, constructed by the caller and never through `Default`, and the loss profile registered through U2. The profile covers the pairs `toolgraph` → `mcp-extended`, `toolgraph` → `compact-signature`, `toolgraph` → `gbnf`, and `toolgraph` → `prompt.<family>`.
- **`graph`:** `ToolGraph::load(bytes, media_type, base, &Vocabulary, governors) -> Result<ToolGraph, LoadError>` produces typed `Tool` values:
  - FnO identity, input and output shapes, and MCP annotations;
  - reified parameter hints (recovery tolerances, taint, session binding), read from reifier side tables with graph scope kept;
  - the execution profile, context management and the security model.
  
  Load is all-or-nothing. Every out-of-vocabulary term, every shape violation and every unsupported construct is a typed `LoadError` that names subject, predicate and source span. Nothing is skipped.
- **Context management:** the `outputProjectionJq` field-mask subset (`.a.b`, `.[]`, `{a, b}`, `.a[]?`) is parsed at load. Any other jq is refused with `LoadError::UnsupportedProjection` and never ignored.
- **`codec::mcp`:**
  - `encode(&ToolGraph) -> (ManifestJson, LossLedger)` emits `name`, `description`, `inputSchema`, `outputSchema`, `annotations`, `_meta.security`, `_meta.executionProfile` and `_meta.contextManagement`.
  - `decode(&ManifestJson, &Vocabulary) -> (ToolGraph, LossLedger)` is the reverse.
  - **Round-trip law:** `decode(encode(g))` is isomorphic to `g` modulo exactly the ledgered losses. A loss that is not on the ledger is a bug.
  - Output is byte-deterministic: key order is fixed, and nothing depends on time or a random hasher.
- **Bridge to core:** `ToolGraph::tool_set(budget) -> ToolSet` uses the existing MCP adapter with the metadata augmentation specified in PR #42. Add that narrow adapter API while retaining the exact schema and original declaration; current `ToolSet::from_mcp` alone has no alias/effects augmentation. Recovery stays on the existing engine, without rebuilding a lossy typed schema subset.
  - Each reified alias becomes a `ParameterSpec::alias`.
  - `readOnlyHint` and the security model select `Effects` (`ReadOnly` only when the graph proves no write capability).
  - The policy follows from the tolerances.

## Acceptance

- Loading the worked example from TOOLS.md gives the graph that TOOLS.md describes. The MCP Extended JSON golden is byte-identical.
- The round trip holds for the worked example and for a generated property over the vocabulary. Every ledger entry is sound under the registered profile.
- Each `LoadError` variant has a test with the invalid input and a neighbouring valid input.
- The crate builds for `wasm32-unknown-unknown` and appears in the `make wasm-test` build list.
- The dependency closure is registered in `vendor/dependencies.json` and the baseline in the same change, and the network denylist audit passes.
- The production path uses the actual upstream G7 profile; alias recovery, exact unmodeled-schema validation, byte-exact declaration render-back and metadata collision refusal are exercised through the augmented existing adapter.
- `make check`.
