<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Tool graph delivery plan

Status: **pipeline specified; caller-input foundations implemented**. This document and [TOOLS.md](TOOLS.md)
are the Phase 2 deliverable completed in #32. Implementation and necessary
dependency adoption are authorized, without a separate blocking review step.
Resolve and register the actual dependency closure under
[DEPENDENCIES.md](../DEPENDENCIES.md), and preserve the executable acceptance
below throughout delivery.

The unpublished [`purrassist-toolgraph`](../../crates/toolgraph/README.md) crate
implements complete vocabulary/loss-profile construction, field-mask parsing
the complete caller-owned MCP context fragment codec and governed AppArmor glob
syntax/policy words.
It builds one upstream namespace table from validated raw prefix declarations,
so conflicting entries cannot disappear through upstream deduplication.
`FieldMask` has private validated storage and a borrowed `FieldMaskKind` view;
the enum below describes its operations rather than permitting unchecked masks.
`ContextManagement` likewise has private validated storage and read-only getters;
its record below describes the data contract rather than unchecked construction.
Loader, schema bridge, full-manifest codec, policy, projection and host execution contracts
remain planned. Facade/host wiring waits for their complete integration and
registry-resolvable packaging; it is not claimed by the foundation crate.

## Scope and ownership

The implementation has one graph authority and one recovery engine. A new
`purrassist-toolgraph` crate owns the graph and pure compilation. Its modules
are `vocab`, `graph`, `codec::mcp`, `policy`, `apparmor`,
`lanes::{gbnf,compact,projector}`, `routing`, `recovery` and `manifest`.
It uses PurRDF leaf crates, never the `purrdf` facade. It must build for
`wasm32-unknown-unknown`. `purrassist-core` retains its empty dependency closure;
its recovery, schema validation and receipt engine remain the production engine.

`purrassist-host` owns canonical root resolution, authenticated session intake,
result handling and execution. Linux confinement is in
`cfg(target_os = "linux")`; other hosts may inspect and compile a graph but
cannot execute a tool requiring AppArmor. Absence of confinement is a typed
failure. The host registry and `purrassist-cli` own CLI/MCP adapters. The
`purrassist` facade re-exports the graph surface, without adding ambient access
to core. Graph-backed `tools/list` and dispatch replace the registry's independent
`writes`-to-hint mapping. There is no legacy unguarded dispatch alternative.

Sources inspected for this plan are PurRAssist `92aa327` and PurRDF
`e1ad49f83789b479aeb0cdefbddf668b4d9cb099`, dated 2026-10-26. The latter is newer
than reconnaissance #31's `2a982d611`. Current sources expose
`FnoCatalog::from_dataset` in `crates/rdf-core/src/fno/read.rs`,
`LossProfile::extend` in `crates/rdf-core/src/loss/profile.rs`, and
`srl::infer_until` in `crates/shapes/src/srl/document.rs`. Thus gaps G1, G2 and
G5 have source implementations; upstream #747, #748 and #749 are closed.
They are not current issue blockers. A release or dependency resolution containing
those implementations must still be verified before #33/#34. A version string
alone does not establish that it includes the fixes for upstream #747–#749.

## Public model and operation contracts

The following Rust signatures specify the intended public surface; except for
the caller-input foundation above, they are not claims that these types exist. Domain records retain their original graph
terms, graph scope and source spans. Identity types have validated private
constructors, preventing callers from fabricating admitted or executable values.
All ordered collections use canonical identifiers or explicit source order,
never randomized iteration. Numeric ceilings, units and token bounds are integers.

```rust
pub struct SourceSpan { pub source: String, pub start: usize, pub end: usize }
pub struct LocatedTerm { pub subject: String, pub predicate: String,
                         pub graph: Option<String>, pub span: SourceSpan }
pub struct ToolId(String);
pub struct ShapeId(String);
pub struct RuleId(String);
pub struct CheckId(String);
pub struct GraphDigest([u8; 32]);
pub struct BundleId([u8; 32]);
pub struct ReceiptDigest([u8; 32]);
pub struct PrefixTable(pub Vec<(String, String)>);
pub struct Vocabulary { namespaces: purrdf_shapes::json_schema::Namespaces,
                        prefixes: PrefixTable, terms: TermBindings,
                        losses: purrdf_core::loss::LossProfile }
pub struct TermBindings { pub namespace_iri: String,
                          pub local_names: Vec<(String, String)> }
pub struct ToolGraph { original: purrdf_core::RdfDataset,
                       tools: Vec<Tool>, digest: GraphDigest,
                       vocabulary: Vocabulary }
pub struct Tool { pub id: ToolId, pub name: String, pub description: String,
                  pub input: ShapeId, pub output: ShapeId,
                  pub annotations: Annotations, pub parameters: Vec<Parameter>,
                  pub execution: ExecutionProfile, pub security: SecurityModel,
                  pub context: ContextManagement, pub policy: Vec<RuleId> }
pub struct Annotations { pub title: Option<String>,
                         pub read_only_hint: bool, pub destructive_hint: bool,
                         pub idempotent_hint: bool, pub open_world_hint: bool }
pub struct Parameter { pub shape: ShapeId, pub predicate: String,
                       pub hints: Vec<ParameterHint> }
pub struct ParameterHint { pub reifier: String, pub location: LocatedTerm,
                           pub aliases: Vec<String>, pub edit_distance: u32,
                           pub case_fold: bool, pub unicode_fold: bool,
                           pub coercions: Vec<Coercion>, pub taint: Vec<String>,
                           pub session_binding: Option<String> }
pub enum Coercion { None, StringToInteger, IntegerToString }
pub struct ExecutionProfile { pub mode: ConfinementMode,
                              pub profile_name: String, pub hat_name: String,
                              pub subprofiles: Vec<ExecutionProfile>,
                              pub limits: ResourceLimits,
                              pub egress_allowed: bool }
pub enum ConfinementMode { Standalone, Hat }
pub struct ResourceLimits { pub address_space_bytes: u64,
                            pub file_size_bytes: u64, pub processes: u64,
                            pub cpu_seconds: u64, pub open_files: u64 }
pub struct ContextManagement { pub output_projection: FieldMask,
                               pub output_limit: OutputByteLimit,
                               pub history_retention_turns: u64,
                               pub intent_tags: Vec<String>,
                               pub loading: LoadingStrategy }
pub enum LoadingStrategy { Always, OnDemand }
pub enum FieldMask { Path(Vec<String>), Elements,
                     Fields(Vec<String>), OptionalElements(String) }
pub struct SecurityModel { pub files: Vec<FileRule>,
                           pub capabilities: Vec<CapabilityRule>,
                           pub network: Vec<NetworkRule>, pub unix: Vec<UnixRule>,
                           pub signals: Vec<SignalRule>, pub ptrace: Vec<PtraceRule>,
                           pub mounts: Vec<MountRule>, pub imports: Vec<String>,
                           pub children: Vec<ToolId>, pub waivers: Vec<Waiver> }
pub struct Qualifiers { pub owner: bool, pub audit: bool, pub deny: bool }
pub enum FilePermission { Read, Write, Append, Lock, Link, Map }
pub enum ExecTransition { Inherit, Profile(String), Child(String),
                          Unconfined { scrub_environment: bool },
                          ProfileInherit(String), ChildInherit(String) }
pub struct Glob { source: String }
pub struct FileRule { pub path: Glob, pub permissions: Vec<FilePermission>,
                      pub exec: Option<ExecTransition>, pub qualifiers: Qualifiers }
pub struct CapabilityRule { pub name: String, pub deny: bool, pub audit: bool }
pub enum SocketSelector { Type(String), Protocol(String) }
pub struct Endpoint { pub ip: Option<String>, pub ports: Option<(u16, u16)> }
pub struct NetworkRule { pub domain: String, pub selector: SocketSelector,
                         pub access: Vec<String>, pub local: Endpoint,
                         pub peer: Endpoint, pub qualifiers: Qualifiers }
pub struct UnixRule { pub access: Vec<String>, pub socket_type: String,
                      pub address: Option<String>, pub peer_label: Option<String>,
                      pub peer_address: Option<String>, pub qualifiers: Qualifiers }
pub struct SignalRule { pub access: Vec<String>, pub signals: Vec<String>,
                        pub peer: String, pub qualifiers: Qualifiers }
pub struct PtraceRule { pub access: Vec<String>, pub peer: Option<String>,
                        pub qualifiers: Qualifiers }
pub enum MountOperation { Mount, Remount, Unmount, PivotRoot }
pub struct MountRule { pub operation: MountOperation, pub deny: bool,
                       pub source: Option<String>, pub target: Option<String> }
pub struct Waiver { pub rule: RuleId, pub reviewer: String,
                    pub rationale: String, pub source_identity: String,
                    pub source: SourceSpan }
```

`TermBindings` must contain exactly the TOOLS.md term set, each mapped to an
absolute IRI under the caller-owned namespace. Missing, duplicate or conflicting
bindings are errors. It has no `Default`. Annotations distinguish absent
`title` from a supplied title; absent MCP boolean hints normalize according to
MCP defaults (`readOnlyHint=false`, `destructiveHint=true`,
`idempotentHint=false`, `openWorldHint=true`), with original presence retained in
the dataset for round trips. Hints are untrusted descriptions, never authority
to bypass static admission. Glob and execution constructors parse and validate
their strings; unknown permission names and invalid transition targets fail.
`ResourceLimits` values are exact: a byte limit not expressible by the selected
AppArmor parser is refused rather than rounded upward.

```rust
pub struct Ceiling { pub work: u64, pub bytes: u64, pub facts: u64,
                     pub generated_terms: u64, pub rounds: u64,
                     pub output_bytes: u64 }
pub struct Usage { pub work: u64, pub bytes: u64, pub facts: u64,
                   pub generated_terms: u64, pub rounds: u64,
                   pub output_bytes: u64 }
pub struct BudgetExhausted { pub resource: Resource, pub limit: u64,
                            pub consumed: u64, pub usage: Usage }
pub enum Resource { Work, Bytes, Facts, GeneratedTerms, Rounds, OutputBytes }
pub struct Governors<'a> { pub ceiling: Ceiling,
                          pub stop: &'a dyn purrdf_core::StopSignal }
pub struct Projection<T> { pub artifact: T,
                           pub ledger: purrdf_core::loss::LossLedger,
                           pub mandatory_checks: Vec<PostCheck>,
                           pub source_digest: GraphDigest }
pub struct PostCheck { pub id: CheckId, pub tool: ToolId,
                       pub source: LocatedTerm, pub kind: CheckKind }
pub enum CheckKind { InputSchema, OutputSchema, Pattern(String),
                     SessionBinding(String), Taint(String), Policy,
                     Confinement, OutputFieldMask, OutputByteBound,
                     HistoryRetention, Selection }
pub struct AdmittedGraph { graph: ToolGraph, certificate: AdmissionCertificate }
pub struct AdmissionCertificate { pub graph: GraphDigest,
                                  pub policy_hash: [u8; 32],
                                  pub trust_registry_digest: [u8; 32],
                                  pub proof_digest: [u8; 32] }
pub struct TrustedPolicyRegistry { bindings: Vec<TrustedPolicyBinding> }
pub struct TrustedPolicyBinding { pub document_sha256: [u8; 32],
                                  pub tool: ToolId, pub vocabulary_digest: [u8; 32],
                                  pub issuer: String,
                                  pub session_fact_schema_sha256: [u8; 32] }
pub struct SessionFacts { authenticated: purrdf_core::RdfDataset }
pub struct Proof { pub contract_hash: [u8; 32], pub digest: [u8; 32],
                   pub witness: String }
pub enum ExhaustionCause { Limit(BudgetExhausted),
                           Cancelled { native_cause: purrdf_core::StopCause } }
pub enum Authorization { Permitted { proof: Proof, usage: Usage },
                         Denied { rules: Vec<RuleId>, proof: Proof, usage: Usage },
                         Exhausted { cause: ExhaustionCause, usage: Usage } }
```

`SessionFacts` is constructed by the host from authenticated input, not model
output. All errors preserve usage. A cancellation returns exhaustion with its
stop cause; it cannot produce partial success. Exhaustion is never denial or a
proof of impossibility. Overflow while computing ceilings is a typed failure,
not saturating arithmetic that silently loosens the limit. Budget checks precede
allocation, recursion and expansion. `Coercion::None` maps to `"none"` and cannot appear alongside another coercion;
the other variants map exactly to `"string-to-integer"` and
`"integer-to-string"`. `ExecutionProfile::profile_name` and `hat_name` preserve
both RDF fields independently of the selected mode; `subprofiles` preserves the
`pa:subprofile` edges, with cycles rejected. `SecurityModel::children` preserves
`pa:subtool` edges between tools; admission rejects cycles and computes their
security closure separately from execution-profile nesting. Standalone executable identity comes
from the admitted FnO implementation, not an invented profile field.
`ExecTransition::Unconfined { scrub_environment: false }` maps to `ux`, and
`true` maps to `Ux`. Both require a trusted, audited waiver for the exact rule;
`Waiver` preserves `pa:waivedRule`, `pa:reviewer`, `pa:rationale` and
`pa:waiverSource` rather than deriving trust from a model-supplied string.
`ContextManagement::output_limit` will use the canonical positive
`OutputByteLimit`. RDF `pa:maxOutputBytes` and MCP `maxOutputBytes` will preserve
its exact byte value. The MCP context object will additionally require
`outputEnforcement: "exact-bytes"`; the enclosing security bundle will require
`format: "purrassist-toolgraph-v2"`. The codec will reject legacy
`maxOutputTokens`, even alongside the new byte field; missing or unknown modes,
duplicate or unknown context fields, zero or out-of-range limits, and unsupported
bundle versions will fail explicitly. A v1 bundle will not be automatically
migrated. The loss profile identity will remain `purrassist-toolgraph-v1`.

The canonical checker will measure the complete serialized projected result,
including quotes, escapes and JSON framing, using exact UTF-8 bytes. Its
success measurement and `OutputOverBudget` failure will carry unit `Bytes`,
enforcement `ExactBytes`, configured limit and measured quantity. Failure will
retain the original output and will not release a truncated substitute. Exact
byte limits will not accept tokenizer evidence as an override. The context
codec and runtime bridge will call this one implementation; no parallel
counting or projection implementation will be introduced. The standalone
checker alone will not prove the full manifest round trip or host result path.
Those integrations and their executable evidence will remain required before
#47 closes.

An output artifact is published only after
all checks finish. `Projection` carries checks as well as the upstream ledger:
upstream `CompiledSchema.losses` is merged without deleting or inventing entries.
Entries identify their source constraint so repeated codes remain distinguishable.

```rust
impl Vocabulary {
    pub fn new(primary_prefix: &str,
               prefixes: PrefixTable, terms: TermBindings,
               losses: purrdf_core::loss::LossProfile)
        -> Result<Self, VocabularyError>;
}
impl ToolGraph {
    pub fn load(bytes: &[u8], media_type: &str, base: Option<&str>,
                vocabulary: &Vocabulary, governors: &Governors<'_>)
        -> Result<Self, LoadError>;
    pub fn tool_set(&self, governors: &Governors<'_>)
        -> Result<purrassist_core::tool_call::ToolSet, LoadError>;
}
pub fn encode(graph: &ToolGraph, governors: &Governors<'_>)
    -> Result<Projection<ManifestJson>, CodecError>;
pub fn decode(json: &ManifestJson, vocabulary: &Vocabulary,
              governors: &Governors<'_>) -> Result<Projection<ToolGraph>, CodecError>;
pub trait DatalogPolicyGate {
    fn admit(&self, graph: ToolGraph, trust: &TrustedPolicyRegistry,
             governors: &Governors<'_>)
        -> Result<AdmittedGraph, PolicyRefusal>;
    fn authorize(&self, graph: &AdmittedGraph, session: &SessionFacts,
                 tool: &ToolId, arguments: &Arguments,
                 governors: &Governors<'_>) -> Result<Authorization, PolicyError>;
}
pub struct InvocationContext { pub workspace_root: String, pub tool_tmp: String,
                               pub home: String, pub enumeration_ceiling: u64 }
pub trait AppArmorSynthesizer {
    fn synthesize(&self, graph: &AdmittedGraph, context: &InvocationContext,
                  governors: &Governors<'_>) -> Result<PolicyText, SynthesisError>;
}
pub struct GbnfOptions { pub unicode_ranges: u64, pub rules: u64,
                         pub maximum_repetitions: u64 }
pub trait GbnfCompiler {
    fn compile(&self, graph: &ToolGraph, family: ModelFamily,
               options: &GbnfOptions, governors: &Governors<'_>)
        -> Result<Projection<Gbnf>, GbnfError>;
}
pub trait ModelFamilyProjector {
    fn project(&self, graph: &ToolGraph, family: ModelFamily,
               selection: &Selection, governors: &Governors<'_>,
               counter: Option<&dyn TokenCounter>)
        -> Result<Projection<PromptBlock>, ProjectionError>;
}
pub trait TokenCounter {
    fn count(&self, bytes: &[u8]) -> Result<u64, ProjectionError>;
}
pub fn compact(graph: &ToolGraph, depth: u64, governors: &Governors<'_>)
    -> Result<Projection<CompactSignature>, ProjectionError>;
pub struct ToolRouter { corpus: purrdf_text::PreparedCorpus }
impl ToolRouter {
    pub fn new(graph: &ToolGraph, profile: purrdf_text::RankingProfile,
               governors: &Governors<'_>) -> Result<Self, RoutingError>;
    pub fn route(&self, query: &str, k: usize, governors: &Governors<'_>)
        -> Result<Vec<RankedTool>, RoutingError>;
}
pub trait RecoveryBridge {
    fn process(&self, graph: &AdmittedGraph, raw_output: &[u8], family: ModelFamily,
               session: &SessionFacts, projections: &[ProjectionBinding],
               paths: &[CheckedPathWitness], governors: &Governors<'_>) -> Result<GatedCalls, BridgeError>;
}
```

`ManifestJson`, `PolicyText`, `Gbnf` and `CompactSignature` are validated owned
UTF-8 byte wrappers. `Arguments` is the existing lossless core JSON document;
source bytes and duplicate members remain available. `ModelFamily` has variants
`Claude`, `Qwen25`, `Qwen3`, `Qwen3Coder`, `Hermes`, `Llama3`, `Llama4`, `MistralV3`,
`MistralV11`, `DeepSeekV3`, `DeepSeekV31` and `CompactTypeScript`. `PromptBlock`
contains placement (`ToolsField` or `SystemPrompt`), declaration bytes, family
and dialect identity. `Selection` contains ordered tool ids and includes all
`Always` tools. `RankedTool` contains tool id and PurRDF's fixed-point score.
`ProjectionBinding` binds artifact digest, graph digest, family, ledger digest
and checks; the bridge rejects a missing or mismatched binding. A family label
alone does not prove what the model saw.

`CheckedPathWitness` is a privately constructed record binding the argument
JSON pointer and canonical path to graph, root, session and call digests. The
host constructs it after `Store::resolve` succeeds; the pure bridge checks those
bindings without filesystem access.

`GatedCalls` retains every recovered per-call outcome. Each `GatedCall` contains
the original recovery call and receipt, post-check results, authorization,
confinement target and graph/artifact/session bindings. Only a privately
constructed `ExecutableCall` can reach host dispatch. A denied, ambiguous,
exhausted or mutating-guess call has no such value. The bridge itself never
executes, reads paths or invents authentication. Host path checks using the
canonical `Store::resolve` rules precede authorization, and host execution
revalidates the same arguments, root, graph and session binding to prevent stale
or substituted authorization. The pure bridge accepts only the host's checked
path witness for path-valued inputs; absence is `BridgeError::MissingPathWitness`.

The current `ToolSet::from_mcp` adapter sets `Effects::Mutating` and has no
public parameter-alias augmentation or `CompiledTool` mutator. Its exact imported
schema cannot safely be reconstructed through the narrower typed-definition API.
Phase 3 therefore adds this narrow extension to the existing core adapter,
without adding dependencies or a second parsing path:

```rust
pub struct DeclarationMetadata {
    pub tool_name: String,
    pub effects: purrassist_core::tool_call::Effects,
    pub policy: purrassist_core::tool_call::RepairPolicy,
    pub parameter_aliases: Vec<ParameterAliases>,
}
pub struct ParameterAliases {
    pub property_path: Vec<String>,
    pub aliases: Vec<String>,
}
impl ToolSet {
    pub fn from_mcp_with_metadata_governed(
        json: &str, metadata: &[DeclarationMetadata],
        governor: &mut purrassist_core::Governor,
    ) -> Result<Self, purrassist_core::tool_call::ToolSetError>;
}
```

`property_path` is the canonical sequence of object member names to the hinted
parameter, not an unvalidated JSONPath expression. The existing adapter's private
pending-definition/`ParameterSpec` assembly applies `alias`, effects and policy
before the normal tool-set compilation. It retains the original schema source,
all exact schema keywords, original declaration bytes and existing validation
semantics. Legacy `from_mcp` methods invoke the same adapter with empty metadata;
they retain their conservative behavior. There is no public mutation of a compiled
tool and no local rebuilding of an approximate typed schema.

Unknown tools or property paths, duplicate metadata records, alias collisions,
unsupported unmodeled hint paths and invalid metadata are `InvalidDefinition`
or `Collision` errors with the exact tool/path context; exhaustion remains
`ToolSetError::Resource`. Metadata is supplied only by graph binding, never taken
as trusted effects authority from MCP hint fields. Before security admission,
`ToolGraph::tool_set` supplies conservative `Mutating` effects. Once admitted,
the bridge uses metadata derived from the admission certificate to select proven
effects; only a true read-only hint and proven no-write implementation select
`ReadOnly`. Alias metadata does not bypass exact-schema validation. #33 executes
a consumer importing a schema with keywords outside the typed-definition subset,
attaches an alias, recovers through it, rejects a value violating an unchanged
schema constraint and verifies byte-exact source declaration render-back. A
neighbouring collision case must fail. This is required integration acceptance,
not a claim about the current adapter.

Aliases then feed `ParameterSpec::alias`; input/output schemas use existing
compiled schemas and the extended existing MCP adapter. Core `RepairPolicy` has `Structural` and
`Tolerant`, not independent folding or edit-distance switches. The bridge checks
each actual repair against the reified tolerance and refuses disallowed repairs;
it must not claim core offers narrower knobs than it does. A true `readOnlyHint` plus a static proof of no writes selects
`Effects::ReadOnly`; all other tools select `Mutating`.

## Failure algebra

These are the complete initial error variants. Each structured failure includes
the triggering source location, operation, usage and underlying cause where
applicable. String details are diagnostics, not substitutes for stable variants.
No method returns an empty artifact on failure.

| Error type | Variants and semantics |
|---|---|
| `VocabularyError` | `MissingTerm`, `DuplicateTerm`, `InvalidIri`, `ConflictingPrefix`, `LossRegistrationConflict`; construction publishes no partial vocabulary. |
| `LoadError` | `Parse`, `UnknownTerm`, `InvalidShape`, `InvalidFnoBinding`, `InvalidHint`, `InvalidExecutionProfile`, `InvalidSecurityRule`, `UnsupportedConstruct`, `UnsupportedProjection`, `UnresolvedImport`, `ImportCycle`, `MissingPostCheck`, `CoreToolSet`, `Exhausted`, `Cancelled`, `ArithmeticOverflow`; includes subject, predicate, graph and span. |
| `CodecError` | `InvalidJson`, `DuplicateMember`, `UnknownField`, `InvalidManifest`, `UnsupportedVersion`, `UnmappedConstraint`, `Load`, `Exhausted`, `Cancelled`, `ArithmeticOverflow`; decoding is all-or-nothing. |
| `PolicyRefusal` | `StaticConflict`, `ProtectedHead`, `IndeterminateHead`, `UntrustedDecisionFact`, `UntrustedPolicy`, `TrustBindingMismatch`, `InvalidRules`, `UnresolvedImport`, `UnprovableContainment`, `Exhausted`, `Cancelled`, `ArithmeticOverflow`; static conflicts include complete sorted SARIF results and proofs. |
| `PolicyError` | `InvalidSession`, `InvalidArguments`, `InvalidCertificate`, `Evaluation`, `MissingWitness`, `ArithmeticOverflow`; controlled stop/limit during evaluation yields `Authorization::Exhausted`. |
| `SynthesisError` | `InvalidContext`, `InvalidGlob`, `UnboundVariable`, `UnsafeVariableValue`, `InvalidTransition`, `UnenforceableNetworkScope`, `UnrepresentableLimit`, `UnsupportedParserProfile`, `InvalidCertificate`, `UnresolvedAbstraction`, `Exhausted`, `Cancelled`, `ArithmeticOverflow`; refused network sets are never widened or converted to losses. |
| `GbnfError` | `InvalidSchema`, `UnsupportedEnvelope`, `InvalidPattern`, `UnregisteredLoss`, `MissingPostCheck`, `Exhausted`, `Cancelled`, `ArithmeticOverflow`; expressible constraints compile exactly, other admitted constraints require a ledgered check. |
| `ProjectionError` | `UnsupportedFamily`, `UnknownTool`, `InvalidSelection`, `MandatoryToolsOverBudget`, `InvalidTokenCount`, `UnregisteredLoss`, `MissingPostCheck`, `TypeScript`, `Exhausted`, `Cancelled`, `ArithmeticOverflow`; mandatory tools cannot silently disappear. |
| `RoutingError` | `InvalidProfile`, `InvalidQuery`, `UnknownTool`, `Exhausted`, `Cancelled`, `ArithmeticOverflow`; no partial ranking masquerades as a complete result. |
| `BridgeError` | `Recovery`, `ProjectionMismatch`, `MissingPostCheck`, `MissingPathWitness`, `InvalidPathWitness`, `DisallowedRepair`, `PostCheckFailed`, `Policy`, `InvalidCertificate`, `Exhausted`, `Cancelled`, `ArithmeticOverflow`; ordinary per-call nonresolution stays in `GatedCalls`, infrastructure failures fail the operation. |
| `ManifestError` | `InvalidBundle`, `InvalidSignature`, `MissingSignature`, `DigestMismatch`, `MissingArtifact`, `UnregisteredLoss`, `Exhausted`, `Cancelled`, `ArithmeticOverflow`; signature requirements come from caller trust policy. |
| `ConfinementError` | `UnsupportedPlatform`, `AppArmorMissing`, `AppArmorDisabled`, `PolicyNotLoaded`, `UnknownProfile`, `UnknownHat`, `TransitionFailed`, `RestoreFailed`, `InvalidExecutable`, `StaleAuthorization`, `Io`; every variant names a remedy and execution never continues unconfined. |
| `ContextError` | `InvalidOutput`, `ProjectionFailed`, `OutputOverBudget`, `InvalidHistory`, `Exhausted`, `Cancelled`, `ArithmeticOverflow`; original result remains in the receipt even when delivery fails. |

`Exhausted` variants carry `BudgetExhausted`; `Cancelled` carries the native
stop cause and `Usage`. A schema's unsupported construct is distinguished from
resource exhaustion. `UnknownTerm` applies to the admitted vocabulary boundary,
not arbitrary RDF annotations already explicitly supported by the vocabulary.
Named import cycles are refused deterministically; imports are supplied in a
caller-owned document table, never fetched from the network.

## G7: upstream plain tool-schema projection prerequisite

The supported plain profile is adopted in #70, a subdivision of #43. Upstream issue
[PurRDF #788](https://github.com/Blackcat-Informatics/purrdf/issues/788) closed
through [PR #806](https://github.com/Blackcat-Informatics/purrdf/pull/806), pinned
here at `8d0a1c6d9d094e5642ab8af3e96d4262b84a8d77`. The existing
`json_schema::compile` retains its JSON-LD surface; the public
`purrdf_shapes::json_schema::compile_tool_arguments` uses the shared constraint
compiler with an explicit caller-owned `ToolArgumentProfile`.

The adopted profile names both `root_shape_iri` and `root_class_iri` and carries
`ToolFieldBinding` records with `parent_shape_iri`, `predicate_iri` and
`implementation_property`. Input and output profiles each contain their own
used transitive binding closure, including shared nested shapes. Missing,
duplicate, colliding, reserved and unused bindings are native typed refusals.
The same predicate can have different member names under different parents.

The upstream compiler emits bare strings, integers, booleans, objects and
arrays, with exact constraints, requiredness, cardinality and closedness.
The original G7 contract also requires plain JSON numbers. This revision
refuses `xsd:decimal`, `xsd:double` and `xsd:float` with native
`UnsupportedConstruct` / `ToolRepresentationError::Constraint("sh:datatype")`,
retaining the bound member and original predicate. That numeric requirement
remains open under #43 and #33; supported-profile adoption does not complete G7.
Other unsupported RDF representations are likewise explicit refusals.
`CompiledSchema` retains schema/OpenAPI text and the complete source-located
loss ledger. Constraints beyond JSON Schema still require checks against the
retained original SHACL graph. There is no local compiler, JSON-LD rewriting
or reconstruction through the narrower typed tool parameter model.

`ToolSchemaLimits { work, bytes, nodes }` supplies deterministic logical
ceilings. Bytes combine cumulative resident logical charges and native admitted
storage; released resident charges are not refunded. This is not allocator
overhead or complete retained process heap accounting. Preparation remains
caller-owned; this API has no cancellation interface and does not establish
complete core `Governor` governance. Native errors distinguish root/binding and
representation refusals, compilation/model/pattern causes, storage, stack margin,
resource exhaustion and checked arithmetic overflow; refusal returns no artifact.

Run `cargo run -p purrassist-toolgraph --example plain_tool_schemas` for the direct
public consumer. Its shared worked fixtures compare complete expected schemas
for the four worked tools and validate them with the full first-party core
schema engine. The upstream compiler's decoded QueryInput regular expression is
`^customer\u{2d}(?:[0-9])+$`; the proposed manifest bytes remain #33 acceptance. Focused downstream
cases also cover nested input/output objects and arrays, scoped bindings,
required/optional members, closedness, exact integer `xone`/`not`/`someValue`,
loss source subjects, typed refusals and tiny logical ceilings. The legacy
JSON-LD schema/OpenAPI goldens are retained from the previous pin; compiling
plain profiles also preserves the neighboring original JSON-LD schema and
OpenAPI artifacts.

This adopts the supported upstream profile only. Full G7 qualification still
requires exact plain-number semantics upstream and downstream executable
evidence; no local compiler or schema rewriting can satisfy that requirement.
#43 stays open. #33 still owns the actual loader and bidirectional MCP codec, retained source and occurrence spans,
FnO mappings, original-SHACL and policy checking, exact core validation,
facade/host wiring and registry-resolvable packaging. Its complete original
contract stays open until those behaviors are delivered.

## MCP context fragment — implemented

`ContextManagement::{decode_mcp,encode_mcp,encode_json}` implements all six context
fields, requiring the explicit enclosing `purrassist-toolgraph-v2` argument on
both directions. The component uses caller-owned source DOM values, the
validated `FieldMask` and `OutputByteLimit`, portable unsigned quantities and
ordered duplicate-preserving tags. It rejects legacy token fields, unsupported
versions/modes, unknown/duplicate/missing fields, wrong types, invalid quantities
and unsupported masks. All four masks and both loading strategies round trip
with fixed canonical bytes; mathematical integer spellings are not narrowed to
plain decimal lexical forms on import.

The actual `roundtrip_context` example uses this one codec and the shared
governed JSON writer. Numeric retention, exact integer reading/construction and
mask rendering extend existing engines with fallible storage and admitted work.
Source DOM construction is caller-owned. Full enclosing manifests, original RDF
source/provenance, loss-ledger integration and host release remain required
under #33 and #47; this fragment does not establish those acceptance criteria.

## Exact AppArmor byte-language containment — implemented

`GlobLanguage` lowers admitted pathname tokens and complete literal bindings
through the existing core derivative arena/session, using direct governed
postfix `ByteLanguage` construction. This full-match byte driver preserves
arbitrary pathname bytes without routing them through Unicode regex syntax.
Unions, classes, nested/empty alternatives and literal bindings are structural;
they do not expand brace products. Existing Unicode pattern/search behavior
remains unchanged.

Exact inclusion returns `Contained` only after exhausting the reachable product
frontier, or a shortest deterministic counterexample as original bytes. Resource
or cancellation refusal is a separate error. The `glob_containment` consumer
executes this actual production path. Complete permission-set, owner and deny
analysis, trusted graph admission and hat enforcement remain under #34/#35.
No exhausted language search can authorize a tool. See [RESOURCES.md](../RESOURCES.md).

## AppArmor pathname syntax and policy words — implemented

`AppArmorGlob` supplies the first-party byte-oriented syntax model needed by
future synthesis and containment. It retains source, literal/escaped byte
semantics, explicit classes/ranges and brace links, and caller-declared variable
identities. Its governed parser never expands products; deterministic printing
uses one quoted policy word. Literal host binding explicitly refuses unsafe
metacharacters instead of relying on policy quoting as literalization.
Context-sensitive class escapes are refused where the installed AppArmor parser
would reinterpret them; the public Rustdoc defines the admitted subset.

The `apparmor_path_words` consumer emits standalone path rules through this
component. Parser-only qualification does not prove graph admission, matching,
bounded containment, full policy synthesis or confinement. Those requirements
remain under #34/#35; no security decision may use string-prefix comparison as
a substitute. See [RESOURCES.md](../RESOURCES.md) for the resource model.

## Governed JSON emission — implemented

The core's `Node::to_json_governed` and `json::quote_governed` provide the
shared governed emission needed by context and manifest codecs. Their structural
cursor and escaping authority also serve existing convenience/append callers;
the governed driver is iterative and uses fallible ancestor/output storage.
The `emit_context_json` example executes context-shaped JSON emission, without
claiming context-field validation or a complete codec.

The context-fragment implementation above uses governed numeric and mask paths
through their existing implementations. G7, source ownership, full manifest round trips and host result
release remain parent acceptance. Issue #47 remains open until that integration
is verified. See [RESOURCES.md](../RESOURCES.md) for the emission resource model.

## Callable-binding admission — implemented

`CallableBinding::admit` accepts an explicit caller-prepared upstream
`FnoCatalog`, selected mapping index and core `Governor`. It requires a
predicate for each callable parameter/output, exactly one mapping per declared
parameter and exactly one return mapping. Input predicates and implementation
properties must be distinct; input and output occupy separate scopes. JSON
property names are not limited to identifiers or nonempty strings. The admitted
value borrows the original catalog, retaining native blank scopes, literal
facets and implementation identities. A fallible caller-order index supports
direct parameter retrieval and iteration without repeated lookup or cloning.

Catalog lookup uses governed binary search over upstream canonical IRI order.
Pairwise mapping and ambiguity checks are local to one callable's arity and
explicitly quadratic; every scan/comparison and retained index growth is
admitted before work or allocation. Invalid selection, missing predicates,
incomplete/ambiguous mappings, collisions, cancellation and exhaustion return
typed failures without partial successful bindings.

The actual upstream reader is exercised by the `admit_callable` consumer, but
caller preparation and catalog construction are outside this checker's resource
governance. This is neither a second FnO reader nor a loaded graph. Issue #33
retains the complete source admission/ownership contract below, exact occurrence
locations, SHACL/G7 integration, manifest codec and production caller wiring.

## FnO admission view and source ownership

`FnoCatalog::from_dataset` admits its strict default-graph FnO profile, not an
arbitrary mixed tool graph. The loader therefore partitions the retained dataset
into an explicitly declared FnO source graph/view and the vocabulary/SHACL/
reifier/security views. Every source quad and side-table row receives an owner;
unknown or multiply interpreted rows are refused rather than silently discarded.
The FnO view includes the complete catalog/document identity, labels, ordered
parameter/output lists, implementation fields and mapping blank-node structure
required by the upstream reader. It is passed through that reader unchanged
except for the documented, ledgered `fnom:Mapping` alias normalization. This is
a checked projection over parsed terms, not a second FnO parser.

For a named FnO source graph, the declared view projects it to the reader's
default graph while retaining an exact source-graph mapping; do not collapse
distinct graphs with different catalog claims. Reifiers and non-FnO quads stay
in the original dataset and their dedicated checked views, never in the strict
FnO reader input. The view's exclusion of unrelated quads is a scoped projection,
not loss from `ToolGraph`; original bytes, dataset and ownership mapping remain
available to every round trip. Any exported lossy view reports precisely its
omitted source constraints in its ledger. Upstream FnO errors are wrapped as
`LoadError::InvalidFnoBinding` with the original source location. A valid complete
FnO profile and the same profile in the worked mixed graph are neighbouring
acceptance cases; extra unsupported FnO predicates must still fail.

## Security and compiler implementation boundaries

Admission receives a `TrustedPolicyRegistry` from the authenticated host
configuration, separately from the tool graph and model input. The registry has
no implicit default and cannot be populated from a manifest's own assertions.
Each binding authorizes exactly one rule document's original-byte SHA-256 for
one tool, vocabulary digest, named issuer and authenticated session-fact schema.
A changed rule, import closure, tool, vocabulary or session-fact schema invalidates
the binding. Import bytes and their ordered digests are part of the admitted
rule contract, and imported documents require their own matching grants. The
issuer string names the audited grant source; it is not proof by itself. A
valid bundle signature establishes bundle integrity, never rule-authority.
The certificate binds the complete registry digest for execution-time rechecks.

The worked `parameterConstraints` documents that derive `pa:permits` and
`pa:denies` are trusted gate policies only when these bindings match. They are
refused as `UntrustedPolicy` otherwise. Untrusted tool-authored clauses are
admitted separately under `NeverDeriveDeclarations`, protecting the configured
permits, denies and conflict predicates. Their complete allowed intermediate
head set is `pa:candidateAllowed` and `pa:candidateDenied` (both Call → Tool);
trusted gate rules may consume those facts but retain deny precedence and the
independent session/tenant checks. Untrusted datasets cannot assert protected
facts or trusted session facts. The extra heads and the rule body's readable
fact set are checked before evaluation; dynamic or indeterminate heads fail.

Only matching trusted gate programs may derive protected decisions. Applying
the protected-head prohibition to those trusted decision rules themselves would
incorrectly refuse every useful policy. Derivations through variable heads,
imports and intermediate predicates must be checked before execution. The graph
scope of every reified hint is preserved when preparing ephemeral facts.

Runtime uses `srl::infer_until` with limits, explicit evaluation time and entropy
seed, recorded derivations and native usage. Volatile functions and unresolved
imports are rejected at admission. `purrdf-sparql-eval` currently links native
`getrandom` and WASM host clock/entropy imports; these are in the closure below.
Tool-graph compilation and policy execution must not invoke them: use explicit
`InferOptions::with_now` and `with_entropy_seed`, and prove the approved paths
with deterministic repeated runs and trapping host providers. If an upstream
path still requires ambient input, fix it upstream before admitting that path;
there is no local silent fallback. Reachable ambient behavior is a purity defect.

Static conflict closure includes imports and child tools through rules. Hat
containment uses a bounded automaton over the first-party AppArmor glob AST;
a failed containment proof refuses admission. Network scopes are canonical exact
IP addresses or bounded explicit enumeration. Host names and unenumerated CIDRs
are `UnenforceableNetworkScope`. Network access is printed before the domain
and its exclusive socket-type or protocol selector, followed by local and peer conditions. Reject address-family
mismatches and local-only accesses combined with peer conditions, even when a
parser accepts other inconsistent forms. Socket type and protocol are alternatives.
The worked scoped-IP policy declares `abi <abi/4.0-ip>,` and requires its
fine-grained
`network_v8/af_inet` and `network/af_unix` mediation. Offline qualification uses
AppArmor 4.1.7 with explicit matching target features, `--warn=all --Werror` and
`-QK`; record the feature file's SHA-256/provenance and inspect exact scoped DFA
output. Default parser acceptance may omit all network enforcement. An offline
feature file is not evidence of running-kernel support or loaded policy: the
host must verify both and hard-refuse unavailable required mediation. The
original v3/v4 synthesis target contract remains intact; this scoped worked
policy is not evidence that a less capable target can enforce the same scopes.

Unix `addr` conditions name abstract or anonymous sockets. The worked
`@@{TOOL_TMP}/ipc` retains a literal abstract marker before the trusted variable;
with its binding it denotes `@/run/purrassist/session-17/ipc`. The corresponding
peer address ends in `/host`. A sole variable prefix loses the leading slash
during abstract-address compilation. Filesystem Unix sockets require file rules;
validate expanded addresses and never reinterpret a filesystem path silently.

The dangerous-capability deny set and file/mount baseline are TOOLS.md's
normative set. Abstractions come only from a pinned, inspected caller document
closure and render with current `include <abstractions/name>` syntax; an
uncontrolled host include must not introduce unreviewed permissions.

GBNF lowering reads `CompiledSchema`; required object keys precede optional keys
in source schema order. It lowers bounded arrays, string bounds, enums, constants,
integer ranges and alternations. For `oneOf`, overlapping alternatives require a
ledgered exclusivity post-check. Regex HIR lowering uses `regex-syntax`, with
explicit Unicode-range and expansion ceilings; unsupported regex constraints
are ledgered, not erased. Fractional bounds, `multipleOf`, `uniqueItems`,
cross-field predicates, session binding and ownership remain mandatory checks.
Family envelopes come from existing `CompiledGrammar` productions. If that API
cannot expose the needed production, extend the existing core grammar API;
never add a second handwritten envelope implementation. Prompt declarations
reuse `DeclarationForm`, and compact output reuses the TypeScript lane.

Prompt projection will distinguish its exact byte measurement from a
conservative token upper bound. A caller-supplied deterministic `TokenCounter`
may tighten the latter with recorded tokenizer identity and exact counted
representation; it will not override an explicit byte ceiling. The byte-length
fallback will claim a token bound only for an explicitly admitted byte-level
encoding with at most one token per input byte, no uncounted implicit special
tokens, and no representation-expanding preprocessing. All added framing will
be included. Without those assumptions or a verified applicable counter, token
accounting will fail explicitly. Exact bytes will not depend on a tokenizer.
The prompt byte ceiling will include placement framing and selected declarations.
Mandatory `Always` tools over budget will cause failure; omitted optional tools
will receive named selection losses.

Tool-result handling will instead enforce `OutputByteLimit` exactly, after
output validation and field-mask application. History removal and field-mask
application will be recorded transformations. The complete returned serialized
representation, including JSON framing, must fit; a larger result will be
refused while its original remains in the receipt. Neither an exact token
count nor a conservative token upper bound will substitute for this byte check.

The host uses `aa-exec -p purrassist.tool.<name> -- <cmd>` for standalone tools.
A dedicated worker enters a hat through the documented `/proc/self/attr/current`
`changehat <token>^<name>` protocol; verify exact bytes against the installed
libapparmor source before implementing, including restoration. No FFI or unsafe
code is required. Hats mediate a worker's kernel accesses; they do not isolate
in-process memory or already-open descriptors. Only trusted handlers run in hats;
untrusted executable code uses standalone processes with an explicit descriptor
allowlist. Before entering, close or deny inherited sensitive descriptors, prohibit
cross-tool worker reuse with stale authority, and bind the parent profile.
Failed restoration poisons the worker and forbids subsequent dispatch.
PurRAssist emits policies but never loads them into the kernel automatically.

Manifest construction packs original graph, canonical graph digest, compiled
artifacts, ledgers, checks and schema/profile versions through `purrdf_gts`.
Its BLAKE3 bundle identity is verified with `verify_file`; optional Ed25519
COSE_Sign1 signing uses caller-provided keys. Receipt v3 SHA-256 continues to bind
the recovery receipt, call digest, canonical graph, bundle id, projection digests,
post-checks, policy proof and usage, and final confinement target. Any receipt
wire change follows the existing versioning rule and updates the receipt schema
and executable examples. SARIF is a deterministic projection of the same record,
with driver name `purrassist`, built from the public model types rather than the
PurRDF-specific driver builder. Gated authorization is distinct from successful
confined execution, and the final receipt records both outcomes.

## Dependency closure to resolve and register

The dependency proposal is seven direct leaf crates: `purrdf-core`, `purrdf-rdf`,
`purrdf-shapes`, `purrdf-datalog`, `purrdf-text`, `purrdf-gts` and
`purrdf-validate`, plus a direct `regex-syntax` edge for HIR lowering. The new
crate also depends on the existing local `purrassist-core`. No provider SDK,
model runtime, HTTP client or async runtime is proposed. This inspected inventory
guides adoption; implementation must update the actual dependency registries.

The exact inspected graph was resolved offline using a standalone Cargo probe
with those seven path dependencies at PurRDF commit
`e1ad49f83789b479aeb0cdefbddf668b4d9cb099` and `regex-syntax = "=0.8.11"`.
`cargo metadata --offline --format-version 1` resolved 22 PurRDF packages and
53 external registry packages. The inventory is the union of normal and build
edges for all targets, including build-only and platform-specific edges; it is
not a claim that every package is compiled on every target. Dependency tests and
benches are excluded because consumers do not build them. Cargo's resolved graph
retains `aho-corasick` and `memchr` even with regex literal-prefilter features
disabled; inventory registration must still include them. No names matched the
repository network denylist. Checking repository URLs for words such as `smol`
is not a package denylist check: `fastrand` is allowed by that denylist.

All PurRDF packages below report version `3.0.1`. At implementation intake,
resolve a published release containing the three upstream fixes, or use one
consistent exact Git revision across the seven direct leaf dependencies. Do not
silently resolve the older registry `3.0.1` instead of the inspected source.
Reresolution must account for changes to the inspected closure;
publication additionally requires registry-resolvable dependencies and packaging
evidence, not just a local path build.

| PurRDF package | Role in the selected closure |
|---|---|
| `purrdf-core` | IR, FnO reader, canonicalization and loss profile |
| `purrdf-rdf` | RDF 1.2 parsing/serialization |
| `purrdf-shapes` | SHACL, JSON Schema, TypeScript and SRL |
| `purrdf-datalog` | Governed rule evaluation and proofs |
| `purrdf-text` | Fixed-point BM25F routing |
| `purrdf-gts` | Bundle transport, BLAKE3 ids and COSE |
| `purrdf-validate` | Public SARIF/report model |
| `purrdf-cdt` | Composite RDF datatype values |
| `purrdf-deflate` | Gzip/DEFLATE support reached by GTS |
| `purrdf-ed25519` | Signature primitive reached by GTS |
| `purrdf-entail` | Entailment/report dependencies |
| `purrdf-events` | RDF event protocol |
| `purrdf-geo-kernel` | Shape/evaluator report dependency |
| `purrdf-hash` | Deterministic hash primitives |
| `purrdf-iri` | IRI and language-tag validation |
| `purrdf-lex` | Lexical and JSON/CBOR primitives |
| `purrdf-slice` | Shape/evaluator transitive dependency |
| `purrdf-sparql-algebra` | Query/rule syntax |
| `purrdf-sparql-eval` | SRL/query evaluation |
| `purrdf-sparql-results` | Query result and report types |
| `purrdf-stack` | Target-specific stack governance |
| `purrdf-xsd` | Datatypes and exact scalar operations |

The following table lists every external registry package, including packages
already registered in PurRAssist. It supplies exact `name@version` rows for both
`vendor/dependencies.json:cargo_packages` and
`scripts/data/dependency-baseline.json:third_party_packages`. Each row is a
separate registration; groups must not conceal new versions.

| Exact package row | License expression | Registration delta |
|---|---|---|
| `aead@0.6.1` | MIT OR Apache-2.0 | Add row |
| `aes@0.9.3` | MIT OR Apache-2.0 | Add row |
| `aes-gcm@0.11.1` | Apache-2.0 OR MIT | Add row |
| `aho-corasick@1.1.5` | Unlicense OR MIT | Existing row |
| `block-buffer@0.12.1` | MIT OR Apache-2.0 | Add row |
| `bumpalo@3.20.3` | MIT OR Apache-2.0 | Add row |
| `cfg-if@1.0.5` | MIT OR Apache-2.0 | Add row |
| `cipher@0.5.2` | MIT OR Apache-2.0 | Add row |
| `cmov@0.5.4` | Apache-2.0 OR MIT | Add row |
| `const-oid@0.10.2` | Apache-2.0 OR MIT | Add row |
| `cpubits@0.1.1` | MIT OR Apache-2.0 | Add row |
| `cpufeatures@0.3.1` | MIT OR Apache-2.0 | Add row |
| `crossbeam-deque@0.8.8` | MIT OR Apache-2.0 | Add row |
| `crossbeam-epoch@0.9.21` | MIT OR Apache-2.0 | Add row |
| `crossbeam-utils@0.8.23` | MIT OR Apache-2.0 | Add row |
| `crypto-common@0.2.2` | MIT OR Apache-2.0 | Add row |
| `ctr@0.10.1` | MIT OR Apache-2.0 | Add row |
| `ctutils@0.4.6` | Apache-2.0 OR MIT | Add row |
| `digest@0.11.3` | MIT OR Apache-2.0 | Add row |
| `either@1.19.0` | MIT OR Apache-2.0 | Add row |
| `fastrand@2.5.0` | Apache-2.0 OR MIT | Add row |
| `getrandom@0.4.3` | MIT OR Apache-2.0 | Add row |
| `ghash@0.6.0` | Apache-2.0 OR MIT | Add row |
| `hashbrown@0.17.1` | MIT OR Apache-2.0 | Existing row |
| `hybrid-array@0.4.15` | MIT OR Apache-2.0 | Add row |
| `inout@0.2.5` | MIT OR Apache-2.0 | Add row |
| `libc@0.2.191` | MIT OR Apache-2.0 | Add row |
| `memchr@2.8.3` | Unlicense OR MIT | Existing row |
| `once_cell@1.21.4` | MIT OR Apache-2.0 | Add row |
| `polyval@0.7.3` | Apache-2.0 OR MIT | Add row |
| `proc-macro2@1.0.107` | MIT OR Apache-2.0 | Existing row |
| `quote@1.0.47` | MIT OR Apache-2.0 | Existing row |
| `r-efi@6.0.0` | MIT OR Apache-2.0 OR LGPL-2.1-or-later | Add row |
| `rayon@1.12.0` | MIT OR Apache-2.0 | Add row |
| `rayon-core@1.13.0` | MIT OR Apache-2.0 | Add row |
| `regex@1.13.1` | MIT OR Apache-2.0 | Existing row |
| `regex-automata@0.4.18` | MIT OR Apache-2.0 | Existing row |
| `regex-syntax@0.8.11` | MIT OR Apache-2.0 | Existing row |
| `rustversion@1.0.23` | MIT OR Apache-2.0 | Add row |
| `sha2@0.11.1` | MIT OR Apache-2.0 | Add row |
| `structured-zstd@0.0.49` | Apache-2.0 | Add row |
| `syn@3.0.9` | MIT OR Apache-2.0 | Add row |
| `twox-hash@2.1.5` | MIT | Add row |
| `typenum@1.20.1` | MIT OR Apache-2.0 | Add row |
| `unicode-ident@1.0.27` | (MIT OR Apache-2.0) AND Unicode-3.0 | Add row |
| `universal-hash@0.6.1` | MIT OR Apache-2.0 | Add row |
| `visual-hashing@1.0.0` | MIT OR Apache-2.0 OR MulanPSL-2.0 | Add row |
| `wasm-bindgen@0.2.129` | MIT OR Apache-2.0 | Add row |
| `wasm-bindgen-macro@0.2.129` | MIT OR Apache-2.0 | Add row |
| `wasm-bindgen-macro-support@0.2.129` | MIT OR Apache-2.0 | Add row |
| `wasm-bindgen-shared@0.2.129` | MIT OR Apache-2.0 | Add row |
| `windows-link@0.2.1` | MIT OR Apache-2.0 | Existing row |
| `windows-sys@0.61.2` | MIT OR Apache-2.0 | Existing row |

Register all 22 PurRDF `name@3.0.1` rows as well: these are new dependencies
from another repository even though their publisher is the same organization.
Retain their published license expressions, including Unicode-3.0 for core and
IRI and the additional MIT/BSD-3-Clause/Unicode-3.0 terms for lex. The common
PurRDF expression is `MIT OR Apache-2.0 OR MulanPSL-2.0`. Retain exact notices
for every added package under `vendor/licenses/<name>-<version>/`, with upstream
repository, package source/checksum and license provenance, following the existing
inventory's notice records. No source code is relicensed. The table's external
license expressions come from resolved Cargo metadata, not an independent audit
of every source file; retained license files must be checked during registration.

`rayon`/crossbeam support deterministic parallel work; SHA-2 supports canonical
identities and Ed25519 SHA-512; `hashbrown` supports PurRDF interners; regex
packages support SHACL/schema patterns; `aes-gcm` and its RustCrypto closure are
unconditional GTS transport dependencies, not a request to encrypt tool arguments;
`structured-zstd`, `fastrand`, `twox-hash` and `visual-hashing` are reached by the
GTS container. `getrandom`, `libc`, `windows-sys` and `windows-link` are
platform-conditioned evaluator/stack support; the WASM binding/proc-macro closure
is reached by the evaluator's target dependency. These are concrete closure
costs, not independently optional features of the tool graph.

The implementation PR must regenerate the whole resolved inventory, including
any concurrently retained older versions (`syn@3.0.6` versus `syn@3.0.9`,
`unicode-ident@1.0.26` versus `unicode-ident@1.0.27`), rather than replace a row
that an existing caller still uses. `make hygiene` checks graph equality and
notices. Preserve the network denylist exactly as defined in DEPENDENCIES.md and
prove core's empty closure independently. This documentation PR changes no
Cargo manifest, lockfile, registry or dependency baseline.

## Delivery units and executable acceptance

Each issue has a maximum four-hour delivery budget including qualification and
landing. Split before that limit if needed, at a complete public behavior; retain
the parent requirement and dependencies. No partial loader, unsafe execution
path or deliberately skipped check closes an issue. #38 remains the implementation
tracker until all five children are complete; #40 remains open through #39.

| Issue / prerequisite | Complete behavior | Focused executable acceptance before its full gate |
|---|---|---|
| #32 | Reviewed specification and plan | Extract every Turtle/SRL/JSON/GBNF/AppArmor block from TOOLS.md. Parse Turtle through `purrdf_rdf::parse_dataset` as Turtle 1.2, SRL through `srl::check`, JSON through a strict parser, GBNF through llama.cpp's grammar parser or an explicitly recorded equivalent, and AppArmor through parser 4.1.7 with explicit `4.0-ip` target features, `--warn=all --Werror` and `-QK` without loading policy. Record parser version, feature-file SHA-256/provenance and exact IP/port/abstract-address DFA evidence; default parser success is insufficient. Both documents pass `prose-check`. Specification delivery is complete; implementation is authorized. |
| #33; completed #32, upstream #747/#748 and G7 | Pure graph loader and bidirectional MCP codec | Worked example loads to exact typed graph and manifest golden; original graph/reifier scopes preserved; `decode(encode(g))` dataset isomorphism modulo exactly the sound registered ledger on example and generated vocabulary corpus; every `LoadError` variant has invalid input plus neighbouring valid input; v2 manifest round trips preserve exact byte units and enforcement mode, while legacy token fields, v1 bundles, absent/unknown modes and duplicate context fields fail explicitly; field-mask refusals and direct production metadata-augmented `ToolSet` caller exercised, including alias recovery, unchanged unmodeled-schema validation and exact declaration render-back; upstream G7 plain-profile caller tests with exact bare input/output scalar/object/array/closedness semantics and retained original SHACL validation; all-target dependency registry audit; `cargo build -p purrassist-toolgraph --target wasm32-unknown-unknown`; add crate to required WASM build list. |
| #34; #33, upstream #749 | Static security admission and governed authorization | One negative plus valid neighbour per conflict class (read-only/write and mount, forbidden egress including unix peers, unconfined transitions, dangerous capabilities, hat containment); deterministic SARIF including both source rules and proof; matching user/active tenant permit, foreign user/inactive tenant deny; protected decision-head injection refused; missing or mismatched policy trust registry binding refused and matching binding admitted; tiny ceiling and pre-tripped stop exhaust without permit/deny; deterministic proof/usage and explicit-time/entropy paths. |
| #35; #33/#34 | Deterministic policy synthesis and Linux confined execution | Worked profiles/hats byte-exact, every golden parsed with AppArmor 4.1.7, explicit `4.0-ip` target features and `--warn=all --Werror -QK`, with exact scoped DFA evidence; absent parser visibly reports named unmet parser evidence; `/8` refusal and exact-host valid neighbour; root containing `{`, `*` and `@{` escaped or refused; each host failure has typed remedy and proves no invocation occurred; operator-loaded smoke test denies SSH secrets and allows root reads/writes; failed restoration poisons worker. |
| #36; #33 | GBNF, compact/prompt lanes and deterministic routing | Worked goldens for every family and complete registered ledgers; parser accepts every GBNF; generated accepted JSON validates or fails only named ledgered constraints, including `oneOf` overlap and noninteger numeric restrictions; exact current declaration/envelope reuse; fixed-point route order and id tie-break; complete prompt bytes at most the explicit byte ceiling, distinct exact-byte and conservative-token receipt fields, refusal of token-bound claims without applicable tokenizer assumptions, mandatory-tool over-budget refusal, named optional omission losses. |
| #37; #34/#35/#36 | Real recovery → post-check → authorization → confined dispatch path | Misspelled tool/stringified integer recovery stays within hint tolerances; pattern failure caught; foreign customer denied with proof; valid call has pinned receipt; mutating guesses never executable; deleted post-check fails at load; root/path and stale witness failures prevent execution; manifest graph/artifact substitution fails; field masks and history losses recorded; complete projected result bytes obey the exact limit with Bytes/ExactBytes/limit/measured receipts; a result exceeding N bytes with deterministic token count at most N is refused, exactly N bytes is accepted by this check, and N + 1 bytes fails without releasing truncated output; graph-backed registry includes metadata; `gate_tool_calls` fixture executes identically through CLI and MCP, and generated `docs/TOOLS.md` still matches registry. |
| #39; #38 | Cross-cutting conformance qualification | Full vocabulary and v2 byte-policy manifest round trips; legacy-token/mode rejection and result-byte boundary witnesses through the actual host/CLI/MCP path; separate prompt-token assumption evidence; remove emitted constraint mutation detected by ledger checks; per-family GBNF goldens and accepted-value property; AppArmor/SARIF static negatives and `/8` refusal; repeated/reordered-triples byte identities; named worked-example artifact transcript in `make wasm-test` equals frozen native bytes; enabled-host AppArmor smoke record and exact environment in `docs/QUALIFICATION.md`. |

Every implementation child runs the repository's mandatory `make check`, with
warnings as errors and normal commit hooks/signing. Use focused real-consumer
checks while editing; do not duplicate the full gate immediately before an
imminent hook that already runs it. `make wasm-test` is additionally mandatory
for #39. WASM semantic tests are not replicated indiscriminately: the new shared
transcript proves named native/WASM artifact-byte identities. General grammar,
security, refusal and conformance cases stay native. AppArmor kernel enforcement
cannot be proven by parse-only checks or WASM execution.

The Phase 2 GBNF checker must parse the same grammar language and reject malformed
rules, unresolved references and invalid ranges; merely matching balanced quotes
or accepting generated strings is not equivalent to a grammar parser. A recorded
equivalent states implementation identity and supported constructs. The eventual
GBNF acceptance property also needs a recognizer over emitted grammars; a syntax
parser alone cannot prove that accepted JSON obeys the schema.

Kernel policy loading, changing AppArmor service state and other host lifecycle
operations require separate authorization. A host without an enabled and
operator-loaded policy cannot supply the confinement smoke-test evidence; record
that criterion as unmet and obtain the actual eligible-host run before claiming
#39 complete. Documentation distinguishes parser success, static admission,
authorization and actual kernel enforcement throughout.

## Completion record

The acceptance owner records the exact checks, source revisions, tool versions,
results and remaining limits in the existing queue/PR, without inventing a second
qualification ceremony. Specification delivery in #32 is complete. Continue
authorized implementation, resolve substantive findings and run required gates.
After implementation and conformance land, verify actual issue closures and preserve
all remaining parent acceptance until it is proved.

## Standalone file fragments — implemented

`FileRule` admits the complete standalone subset documented by its public API:
unique `rwaklm` access (write excludes append), optional `ix`/`px`/`cx`/`Pix`/`Cix`,
literal ASCII directed targets where permitted, and audit/deny/owner qualifiers.
Denied `ix` canonicalizes to native bare `x`, retaining the original request and
source identity. Targets on deny rules, other denied transitions and unconfined
`ux`/`Ux` are explicit refusals; an unauthenticated caller cannot waive them.
Path admission uses the original parsed lexical prefix and existing exact
absolute bindings. The public binding renderer and shared governed formatter
produce one atomic native fragment. `file_rules` is its executed consumer.

The selected ABI/file-mediation requirement is not host attestation. This
standalone component does not establish graph admission, permission/owner/deny
interaction analysis, hat containment, complete native profile-name handling,
trusted scoped waiver admission, full synthesis or actual confinement. Original
#34/#35 acceptance remains required.

## Standalone signal and ptrace fragments — implemented

`SignalRule` and `PtraceRule` admit explicit closed access sets and mandatory
peer globs/bindings, preserving independent audit/deny qualifiers. Signal sets
cover the documented 33-name table plus inclusive `rtmin+0` through `rtmin+32`;
canonical ordering, deduplication and leading-zero RT normalization retain the
original field text. Unknown names and native aliases outside the frozen table
are refused. Output always includes the requested accesses, signal set where
applicable, and the quoted peer through the shared native policy-word writer.

These labels use native AARE semantics, without file slash normalization or
Unix-address interpretation. Explicit broad or nullable expressions remain
caller requests, never defaults. The atomic artifact carries ABI `3.0` and its
signal/ptrace mediation requirement; it is neither a peer authorization nor
running-kernel/loaded-policy evidence. `ipc_rules` is the public consumer.
Full #34/#35 graph admission, static conflicts, permission/hat analysis, complete
synthesis, trust and host confinement remain required. Mount/capability/rlimit
and import handling remain separate original requirements.
