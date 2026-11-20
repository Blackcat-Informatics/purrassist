# Issue #33: Tool graph: RDF 1.2 loader, FnO/SHACL binding and MCP Extended JSON codec with LossLedger

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github
Labels: enhancement

## Body

## Objective

Create `purrassist-toolgraph` and its foundation:
- a loader from RDF 1.2 Turtle to a typed, validated tool graph;
- the FnO and SHACL binding;
- a **bidirectional** codec between the tool graph and MCP Extended Tool Manifest JSON, where every projection returns a `purrdf_core::loss::LossLedger`.

Built to `docs/tool-graph/TOOLS.md` and `TOOLS_PLAN.md`; specification delivery in #32 is complete.

## Depends on

- #32 specification delivered. Implementation and necessary dependency adoption are authorized; there is no separate blocking review or authorization step.
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
- The crate builds and passes its native tool-graph tests.
- The dependency closure is registered in `vendor/dependencies.json` and the baseline in the same change, and the network denylist audit passes.
- The production path uses the actual upstream G7 profile; alias recovery, exact unmodeled-schema validation, byte-exact declaration render-back and metadata collision refusal are exercised through the augmented existing adapter.
- `make check`.

## Bounded delivery mapping

- #44 / PR #45 supplies the lossless core MCP metadata overlay; this parent retains actual graph-loader integration.
- #46 supplies validated caller-owned vocabulary/loss profiles, field-mask parsing and the internal unpublished crate. Exact Git dependencies keep it outside the publishable facade until registry-resolvable packaging qualifies. This parent retains facade/host wiring.
- #43 / upstream #788 retains the complete admitted plain-argument compiler and actual downstream consumer. Upstream #790 separately repairs a shared maximal-length overflow; it does not replace that compiler contract.
- Complete typed loading, accounted FnO views, scoped source/reifier ownership, G7 projection, deterministic bidirectional MCP codec and all original round-trip/qualification requirements remain here.

Historical approval-stop language in comments is superseded by the user's explicit correction: "THERE IS NO BLOCKING REVIEW STEP", "STOP MANUFACTURING STOPS", and "AUTHORIZATION WAS NEVER NEEDED". Continue authorized implementation and required verification.


## Output unit acceptance (#47)

The MCP context codec must use the canonical exact result byte contract from #47: `maxOutputBytes`, `outputEnforcement: "exact-bytes"`, and outer bundle format `purrassist-toolgraph-v2`. Full manifest round trips must preserve the byte unit and enforcement mode. Reject legacy `maxOutputTokens` even alongside the new field, v1 bundles, missing/unknown modes, duplicate/unknown context fields and invalid byte bounds; never migrate token numbers silently. Use the same validated `OutputByteLimit` and preserve complete original graph context.



## Callable binding admission and source-governance evidence

#50 supplies stricter callable-binding admission over an already-admitted native FnO catalog: complete ordered parameter/return mappings, unambiguous property and predicate bindings, original native identities, and governed retained indexing. This parent retains the actual loader/G7 consumer.

At pinned `2ed9845`, `FnoCatalog::from_dataset` has no governor or cancellation input and internally collects rows and allocates owned indexes/catalog records. `parse_dataset_with` exposes source-span tracking but no work/storage/stop controls. Its `SpanTable` contains first positions and ordered positions keyed by lexical subject, without a per-quad/predicate/graph occurrence identity. These APIs alone do not establish this issue's governed loading and exact source-location contract. Resolve the actual upstream seams before claiming those guarantees; an outer size check, subject-position fallback or governed DatasetView does not prove admission of reader/parser-owned work and allocations. The standalone #50 operation does not claim governed catalog construction or source parsing. All original source, SHACL, codec and round-trip acceptance remains required.


## Comments (5)

### paudley — 2026-10-26T20:50:04Z

New verified prerequisite G7 from the #32 specification review: current `purrdf_shapes::json_schema::compile` emits JSON-LD schemas, including compacted predicate keys, RDF value carriers and class-root definitions. It cannot directly produce the planned plain MCP argument objects. Simple property-key rewriting would leave other semantics incorrect.

Before this loader/codec delivery, upstream must provide a caller-selected plain FnO argument projection profile with explicit shape/class roots, complete predicate-to-implementation-property bindings, exact scalar/object/array/cardinality/closedness semantics, resource limits, and preservation of all schema constraints and loss entries. The specification plan proposes `compile_tool_arguments` for this contract. It is not implemented, and no local replacement schema compiler is planned. Acceptance must exercise that actual upstream API and retain complete original SHACL validation.

Existing upstream prerequisites #747, #748 and #749 are closed. G7 and the recorded maintainer approval of #32 remain prerequisites; this comment preserves the original behavior rather than substituting JSON-LD arguments.


### paudley — 2026-10-27T05:52:15Z

#54 isolates the complete bidirectional MCP context-management fragment after #52 shared governed emission. It will validate all six fields under an explicit enclosing `purrassist-toolgraph-v2` format, preserve exact byte units/mode and mathematical integer values, and extend existing numeric/mask engines for resource governance. Source JSON DOM construction is caller-owned.

Full RDF loading, manifest round trips, native loss-ledger integration, source provenance and actual host result release remain parent acceptance. This fragment will not establish those guarantees; the parent stays open.


### paudley — 2026-10-28T18:35:40Z

Delivered #122 in PR #124: the actual core schema compiler now constructs byte-identical bundle identities with allocation-free framing, admitted hashing work and a final cancellation check before publication. The plain FnO consumer, six focused identity/budget tests, required WASM libraries, release-compiler lint and settled make check passed.

This completes the bundle-identity prerequisite only. Full source parsing, schema index/reference ownership, algebra preparation, graph admission and #33/#36 integration remain required. #123 is implementing the next complete shared reference-resolution operation; its execution evidence is still pending. Both parent contracts remain open.


### paudley — 2026-11-13T14:40:25Z

Upstream request for the governed-source seam filed as Blackcat-Informatics/purrdf#1306. First bounded delivery is #191 (typed loader), which claims neither governed loading nor exact per-quad source spans.

### paudley — 2026-11-14T14:32:54Z

## Named blocker: the purrdf release that carries the wasm change

Upstream change: purrdf main commit `5403340f` (not tagged, not published). It removes the wasm host imports from `purrdf-sparql-eval`. An embedder installs the clock and entropy once with `purrdf_sparql_eval::host_source::install`, or supplies `with_now` and `with_entropy_seed` per request. With neither on `wasm32-unknown-unknown`, a request gets the typed `EvalError::HostSourceMissing`.

This issue stays blocked until the first purrdf release after 4.0.2 that contains `5403340f` is published and the pin moves to it at the exact version of its siblings.

Our side, once that release is in:
- Install the host source in the wasm harness and in the host, once at startup.
- Add a test that a request without an installed host source gets `EvalError::HostSourceMissing` and not a missing-import failure on instantiation.


