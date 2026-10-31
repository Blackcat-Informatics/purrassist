<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Tool graph specification

Status: **pipeline specified; caller-input foundations implemented**. The
examples and compiler contracts describe Phase 3 acceptance. The existing
recovery/declaration renderers and caller-prepared result projection are
implemented; graph loading, authorization, model-family prompt projection and
confinement are not. Specification delivery is complete in #32;
implementation and necessary dependency adoption are authorized. The delivery
sequence and executable acceptance are in [TOOLS_PLAN.md](TOOLS_PLAN.md).
The unpublished [`purrassist-toolgraph`](../../crates/toolgraph/README.md) crate
implements validated vocabulary/loss-profile inputs and the four-form context
mask parser, with a standalone governed exact output byte-limit checker;
it does not yet load or execute a tool graph.

The governing decisions are #40 D1–D10 and the reconnaissance is #31. No
compiler output authorizes execution. A recovered call passes complete shape
validation, the policy gate and the selected host confinement before dispatch.
A missing check, unsupported constraint or exhausted resource ceiling is an
explicit refusal. No loss ledger licenses a security downgrade.

## Tool-result output units and wire version — specified

The tool-result context contract will enforce one exact byte limit. RDF
`pa:maxOutputBytes`, Rust `OutputByteLimit` and MCP
`_meta.contextManagement.maxOutputBytes` will all denote the maximum UTF-8 byte
length of the complete serialized result after the selected field mask. JSON
quotes, escapes and container framing will count. The positive limit will fit
`u64`; no tokenizer will change this measurement or authorize exceeding it.
The result checker will refuse an oversized representation without truncating
or releasing it, retaining the original output for the receipt.

The MCP Extended bundle format will be `purrassist-toolgraph-v2`. Every tool's
context metadata will require `outputEnforcement: "exact-bytes"` beside
`maxOutputBytes`; the RDF vocabulary's byte property will have that fixed
meaning. The implemented standalone context codec rejects an unsupported supplied bundle
format, an absent or unknown enforcement mode, duplicate fields, and unknown
context fields. The full manifest decoder remains planned. The context codec
rejects legacy `maxOutputTokens` wherever a context limit is expected,
even when `maxOutputBytes` also appears. An imported v1 bundle will fail with
`UnsupportedVersion`; no number will be silently renamed or converted. The
loss profile will remain `purrassist-toolgraph-v1`, because this wire change
does not change its registered loss meanings.

The standalone `OutputByteLimit` checker reports unit `Bytes`, enforcement
`ExactBytes`, the configured limit and measured byte length on success and on
the typed `OutputLimitError::Exceeded` refusal. A result longer than N bytes will fail
this policy even when a deterministic tokenizer counts at most N tokens.
A result of exactly N bytes will pass this check; output shape, authorization
and confinement checks will remain independently mandatory. No exact token
limit or conservative token-limit enforcement mode is specified for tool
results in this version.

Prompt projection has a separate conservative token bound, described in the
model-family matrix. That estimate will not be presented as an exact token
count or as the tool-result byte limit. Issue #47 will remain open until the
manifest round trip and complete runtime result path verify these semantics;
a standalone limit checker will not establish those integration claims.

## Caller-owned vocabulary — specified

The example namespace `https://example.org/toolgraph#` is owned by this example's
caller. Library code receives every term through `Vocabulary` and one caller
`json_schema::Namespaces::new("pa", prefixes)` table; the same complete prefix
table goes to every PurRDF projection. No domain namespace is minted or embedded
in the library. `pa:` and `ex:` below are illustrative bindings, not constants.
Changing a prefix without changing its expansion changes presentation only.
Changing an expansion changes the vocabulary identity and manifest digest.

The following is the complete vocabulary for this version. Every property has
one explicit domain, range and comment. Union domains use the common classes
`pa:Rule` and `pa:Node`; a class is a marker, not an authorization fact. Standard
FnO, SHACL, RDF and OWL properties keep their standards meanings. In particular,
FnO's actual mapping class is `fno:Mapping`; the caller alias `fnom:Mapping` is
accepted only when declared equivalent as below. PurRDF's FnO reader receives
the normalized `fno:Mapping` statements, with the transformation ledgered.

```turtle
@prefix pa: <https://example.org/toolgraph#> .
@prefix ex: <https://example.org/tools/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix fno: <https://w3id.org/function/ontology#> .
@prefix fnom: <https://w3id.org/function/vocabulary/mapping#> .

pa:Node a rdfs:Class ; rdfs:comment "A tool-graph vocabulary node." .
pa:Tool a rdfs:Class ; rdfs:subClassOf fno:Function, pa:Node ; rdfs:comment "A callable FnO function with security and context bindings." .
pa:ExecutionProfile a rdfs:Class ; rdfs:subClassOf pa:Node ; rdfs:comment "One complete confinement contract." .
pa:Context a rdfs:Class ; rdfs:subClassOf pa:Node ; rdfs:comment "Output projection and prompt loading contract." .
pa:Hint a rdfs:Class ; rdfs:subClassOf pa:Node ; rdfs:comment "RDF 1.2 reifier annotating a parameter property shape's sh:path triple." .
pa:Rule a rdfs:Class ; rdfs:subClassOf pa:Node ; rdfs:comment "One typed AppArmor rule; omitted permissions are not granted." .
pa:FileRule a rdfs:Class ; rdfs:subClassOf pa:Rule ; rdfs:comment "AppArmor pathname glob and access or execution transition." .
pa:CapabilityRule a rdfs:Class ; rdfs:subClassOf pa:Rule ; rdfs:comment "One named Linux capability allow or deny." .
pa:NetworkRule a rdfs:Class ; rdfs:subClassOf pa:Rule ; rdfs:comment "AppArmor 4.1 network rule with exact address scope." .
pa:UnixRule a rdfs:Class ; rdfs:subClassOf pa:Rule ; rdfs:comment "Unix socket access and peer mediation." .
pa:SignalRule a rdfs:Class ; rdfs:subClassOf pa:Rule ; rdfs:comment "Signal access, signal set and peer label." .
pa:PtraceRule a rdfs:Class ; rdfs:subClassOf pa:Rule ; rdfs:comment "Ptrace access and peer label." .
pa:MountRule a rdfs:Class ; rdfs:subClassOf pa:Rule ; rdfs:comment "A denied mount, remount, umount or pivot_root operation." .
pa:Waiver a rdfs:Class ; rdfs:subClassOf pa:Node ; rdfs:comment "A host-authenticated narrowly scoped exception for one unconfined exec transition." .
pa:Call a rdfs:Class ; rdfs:subClassOf pa:Node ; rdfs:comment "A request-scoped immutable call identity." .
pa:Session a rdfs:Class ; rdfs:subClassOf pa:Node ; rdfs:comment "Authenticated facts supplied by the trusted host." .
fnom:Mapping a rdfs:Class ; owl:equivalentClass fno:Mapping ; rdfs:comment "Caller alias normalized to the upstream FnO Mapping class." .

pa:name a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range xsd:string ; rdfs:comment "Unique core tool name, independent of display title." .
pa:description a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range xsd:string ; rdfs:comment "Model-visible description." .
pa:inputShape a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range sh:NodeShape ; rdfs:comment "Complete argument SHACL shape." .
pa:outputShape a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range sh:NodeShape ; rdfs:comment "Complete result SHACL shape." .
pa:title a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range xsd:string ; rdfs:comment "MCP annotations.title, verbatim." .
pa:readOnlyHint a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range xsd:boolean ; rdfs:comment "MCP annotations.readOnlyHint, a hint checked against actual declared effects." .
pa:destructiveHint a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range xsd:boolean ; rdfs:comment "MCP annotations.destructiveHint, verbatim." .
pa:idempotentHint a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range xsd:boolean ; rdfs:comment "MCP annotations.idempotentHint, verbatim." .
pa:openWorldHint a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range xsd:boolean ; rdfs:comment "MCP annotations.openWorldHint, verbatim; not a permission." .
pa:waiver a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range pa:Waiver ; rdfs:comment "Audited transition waiver honored only when the host registry matches its source and exact rule." .
pa:waivedRule a rdf:Property ; rdfs:domain pa:Waiver ; rdfs:range pa:FileRule ; rdfs:comment "Exactly one unconfined exec rule covered by this waiver." .
pa:reviewer a rdf:Property ; rdfs:domain pa:Waiver ; rdfs:range xsd:string ; rdfs:comment "Authenticated reviewer verified by caller trust registry." .
pa:rationale a rdf:Property ; rdfs:domain pa:Waiver ; rdfs:range xsd:string ; rdfs:comment "Nonempty explicit security exception rationale." .
pa:waiverSource a rdf:Property ; rdfs:domain pa:Waiver ; rdfs:range xsd:anyURI ; rdfs:comment "Immutable review source bound by host approval record." .
pa:subtool a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range pa:Tool ; rdfs:comment "An explicit potential nested call edge; included transitively in static security closure and independently authorized at runtime." .
pa:executionProfile a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range pa:ExecutionProfile ; rdfs:comment "Mandatory confinement contract." .
pa:contextManagement a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range pa:Context ; rdfs:comment "Mandatory context management contract." .
pa:parameterConstraints a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range xsd:string ; rdfs:comment "Complete SPARQL 1.2 RL document, never executable host code." .
pa:maxEditDistance a rdf:Property ; rdfs:domain pa:Hint ; rdfs:range xsd:nonNegativeInteger ; rdfs:comment "Maximum admitted name or value edit-distance inference; zero forbids it." .
pa:alias a rdf:Property ; rdfs:domain pa:Hint ; rdfs:range xsd:string ; rdfs:comment "Explicit parameter-name alias projected to ParameterSpec::alias." .
pa:typeCoercion a rdf:Property ; rdfs:domain pa:Hint ; rdfs:range xsd:string ; rdfs:comment "Allowed conversion name: string-to-integer, integer-to-string or none." .
pa:caseFold a rdf:Property ; rdfs:domain pa:Hint ; rdfs:range xsd:boolean ; rdfs:comment "Permission for labeled case-folding inferences, never validation normalization." .
pa:unicodeFold a rdf:Property ; rdfs:domain pa:Hint ; rdfs:range xsd:boolean ; rdfs:comment "Permission for labeled Unicode-folding inferences." .
pa:taint a rdf:Property ; rdfs:domain pa:Hint ; rdfs:range xsd:string ; rdfs:comment "A label copied to the candidate's policy facts; not model-controlled." .
pa:sessionBinding a rdf:Property ; rdfs:domain pa:Hint ; rdfs:range rdf:Property ; rdfs:comment "Trusted session predicate to which the argument must be equal." .
pa:confinementMode a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range xsd:string ; rdfs:comment "aa-exec or change_hat; no unenforced mode." .
pa:profileName a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range xsd:string ; rdfs:comment "Exact standalone AppArmor profile name." .
pa:hatName a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range xsd:string ; rdfs:comment "Exact hat inside purrassist.tools." .
pa:egressAllowed a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range xsd:boolean ; rdfs:comment "Whether the profile may grant outbound network access." .
pa:rule a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range pa:Rule ; rdfs:comment "A granular AppArmor rule; order is canonicalized." .
pa:addressSpaceBytes a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range xsd:positiveInteger ; rdfs:comment "set rlimit as in exact bytes." .
pa:fileSizeBytes a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range xsd:positiveInteger ; rdfs:comment "set rlimit fsize in exact bytes." .
pa:processCount a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range xsd:positiveInteger ; rdfs:comment "set rlimit nproc count." .
pa:cpuSeconds a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range xsd:positiveInteger ; rdfs:comment "set rlimit cpu in whole seconds, never fractional seconds." .
pa:openFileCount a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range xsd:positiveInteger ; rdfs:comment "set rlimit nofile count." .
pa:subprofile a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range pa:ExecutionProfile ; rdfs:comment "A named child profile for a cx or Cix transition." .
pa:pathGlob a rdf:Property ; rdfs:domain pa:FileRule ; rdfs:range xsd:string ; rdfs:comment "First-party parsed AppArmor glob including brace alternatives and declared @{VAR}." .
pa:filePermissions a rdf:Property ; rdfs:domain pa:FileRule ; rdfs:range xsd:string ; rdfs:comment "Unique letters from r w a k l m; a and w are mutually exclusive." .
pa:execTransition a rdf:Property ; rdfs:domain pa:FileRule ; rdfs:range xsd:string ; rdfs:comment "Exactly one of ix px cx ux Ux Pix Cix; unsafe ux requires explicit admission." .
pa:transitionTarget a rdf:Property ; rdfs:domain pa:FileRule ; rdfs:range xsd:string ; rdfs:comment "Exact -> profile target where the transition permits it." .
pa:owner a rdf:Property ; rdfs:domain pa:FileRule ; rdfs:range xsd:boolean ; rdfs:comment "Emit the owner qualifier." .
pa:audit a rdf:Property ; rdfs:domain pa:Rule ; rdfs:range xsd:boolean ; rdfs:comment "Emit the audit qualifier." .
pa:deny a rdf:Property ; rdfs:domain pa:Rule ; rdfs:range xsd:boolean ; rdfs:comment "Emit a deny rule; otherwise an allow rule." .
pa:capability a rdf:Property ; rdfs:domain pa:CapabilityRule ; rdfs:range xsd:string ; rdfs:comment "Exact Linux capability name without CAP_." .
pa:domain a rdf:Property ; rdfs:domain pa:NetworkRule ; rdfs:range xsd:string ; rdfs:comment "AppArmor address family such as inet or inet6." .
pa:socketType a rdf:Property ; rdfs:domain pa:Rule ; rdfs:range xsd:string ; rdfs:comment "Socket type such as stream; exclusive with network protocol." .
pa:protocol a rdf:Property ; rdfs:domain pa:NetworkRule ; rdfs:range xsd:string ; rdfs:comment "Protocol such as tcp, exclusive with socketType in AppArmor 4.1." .
pa:access a rdf:Property ; rdfs:domain pa:Rule ; rdfs:range xsd:string ; rdfs:comment "Typed rule access set, comma-separated using grammar-approved tokens." .
pa:localIp a rdf:Property ; rdfs:domain pa:NetworkRule ; rdfs:range xsd:string ; rdfs:comment "One exact local address, without CIDR syntax." .
pa:localPort a rdf:Property ; rdfs:domain pa:NetworkRule ; rdfs:range xsd:string ; rdfs:comment "Port number or inclusive N-M range, within 0 through 65535." .
pa:peerIp a rdf:Property ; rdfs:domain pa:NetworkRule ; rdfs:range xsd:string ; rdfs:comment "One exact peer address, without CIDR syntax." .
pa:peerPort a rdf:Property ; rdfs:domain pa:NetworkRule ; rdfs:range xsd:string ; rdfs:comment "Peer port or inclusive range." .
pa:address a rdf:Property ; rdfs:domain pa:UnixRule ; rdfs:range xsd:string ; rdfs:comment "Unix addr expression in the supported AppArmor glob language." .
pa:peerAddress a rdf:Property ; rdfs:domain pa:UnixRule ; rdfs:range xsd:string ; rdfs:comment "Unix peer addr expression." .
pa:peerLabel a rdf:Property ; rdfs:domain pa:Rule ; rdfs:range xsd:string ; rdfs:comment "Unix, signal or ptrace peer label expression." .
pa:signalSet a rdf:Property ; rdfs:domain pa:SignalRule ; rdfs:range xsd:string ; rdfs:comment "Comma-separated AppArmor signal names." .
pa:mountOperation a rdf:Property ; rdfs:domain pa:MountRule ; rdfs:range xsd:string ; rdfs:comment "mount, remount, umount or pivot_root; deny is mandatory." .
pa:outputProjectionJq a rdf:Property ; rdfs:domain pa:Context ; rdfs:range xsd:string ; rdfs:comment "Only the D6 field-mask language, not arbitrary jq." .
pa:maxOutputBytes a rdf:Property ; rdfs:domain pa:Context ; rdfs:range xsd:positiveInteger ; rdfs:comment "Maximum complete projected output size in exact UTF-8 bytes, including JSON framing." .
pa:historyRetentionTurns a rdf:Property ; rdfs:domain pa:Context ; rdfs:range xsd:nonNegativeInteger ; rdfs:comment "Number of completed turns retained; zero retains none." .
pa:intentTags a rdf:Property ; rdfs:domain pa:Context ; rdfs:range xsd:string ; rdfs:comment "Repeated BM25F intent field values." .
pa:loadingStrategy a rdf:Property ; rdfs:domain pa:Context ; rdfs:range xsd:string ; rdfs:comment "always or on_demand; retrieval never authorizes a call." .
pa:tool a rdf:Property ; rdfs:domain pa:Call ; rdfs:range pa:Tool ; rdfs:comment "Canonical resolved tool identity." .
pa:session a rdf:Property ; rdfs:domain pa:Call ; rdfs:range pa:Session ; rdfs:comment "Trusted immutable session identity." .
pa:userId a rdf:Property ; rdfs:domain pa:Session ; rdfs:range xsd:string ; rdfs:comment "Authenticated user identifier, never read from model output." .
pa:tenant a rdf:Property ; rdfs:domain pa:Node ; rdfs:range pa:Node ; rdfs:comment "Trusted session or customer tenant identity." .
pa:active a rdf:Property ; rdfs:domain pa:Node ; rdfs:range xsd:boolean ; rdfs:comment "Trusted active-tenant status." .
pa:customerId a rdf:Property ; rdfs:domain pa:Call ; rdfs:range xsd:string ; rdfs:comment "Validated customer_id argument projected as a call fact." .
pa:candidateAllowed a rdf:Property ; rdfs:domain pa:Call ; rdfs:range pa:Tool ; rdfs:comment "Untrusted tool-rule intermediate proposal, never sufficient to authorize." .
pa:candidateDenied a rdf:Property ; rdfs:domain pa:Call ; rdfs:range pa:Tool ; rdfs:comment "Untrusted tool-rule intermediate refusal proposal, consumed only by trusted gate rules." .
pa:permits a rdf:Property ; rdfs:domain pa:Call ; rdfs:range pa:Tool ; rdfs:comment "Derived authorization for this exact call and tool." .
pa:denies a rdf:Property ; rdfs:domain pa:Call ; rdfs:range pa:Tool ; rdfs:comment "Derived refusal overriding permits." .
pa:conflict a rdf:Property ; rdfs:domain pa:Node ; rdfs:range xsd:string ; rdfs:comment "Derived static or dynamic contradiction with stable diagnostic code." .
```

`owl:imports` on an execution profile names an AppArmor abstraction by an IRI
resolved in the caller's immutable import table, for example
`urn:apparmor:abstractions/base` maps to `include <abstractions/base>` using
the current include syntax. No network fetch occurs. Imported policy is expanded and checked under the same capability and
network rules; the synthesizer must refuse unknown or over-broad imports.
The default capability allow set is empty. Every generated execution context
also denies `sys_admin`, `sys_ptrace`, `sys_module`, `sys_rawio`, `net_admin`,
`dac_override`, `dac_read_search`, `setuid`, `setgid`, `mknod`, `chown`, `fowner`, `kill` and
`net_bind_service` explicitly.
Additional allows cannot override these denies.

### Vocabulary validation — specified

The next standalone shapes document is the executable structural admission
boundary. Its `sh:targetClass` declarations are supplemented by explicit
validation targets for every graph node matching the loader's recognized
role, so omission of `rdf:type` cannot evade validation. Semantic admission
then validates the remaining cross-node invariants listed below. Unknown
vocabulary predicates, duplicate single-valued properties, unresolved imports
and unbound variables are errors, with source locations, before compilation.

```turtle
@prefix pa: <https://example.org/toolgraph#> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix fno: <https://w3id.org/function/ontology#> .
pa:ToolShape a sh:NodeShape ; sh:targetClass pa:Tool ;
 sh:property [ sh:path pa:name ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ; sh:pattern "^[A-Za-z0-9_.:-]+$" ] ;
 sh:property [ sh:path pa:subtool ; sh:class pa:Tool ] ;
 sh:property [ sh:path pa:description ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:inputShape ; sh:minCount 1 ; sh:maxCount 1 ; sh:class sh:NodeShape ] ;
 sh:property [ sh:path pa:outputShape ; sh:minCount 1 ; sh:maxCount 1 ; sh:class sh:NodeShape ] ;
 sh:property [ sh:path pa:executionProfile ; sh:minCount 1 ; sh:maxCount 1 ; sh:node pa:ExecutionShape ] ;
 sh:property [ sh:path pa:contextManagement ; sh:minCount 1 ; sh:maxCount 1 ; sh:node pa:ContextShape ] ;
 sh:property [ sh:path pa:parameterConstraints ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:title ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:readOnlyHint ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:boolean ] ;
 sh:property [ sh:path pa:destructiveHint ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:boolean ] ;
 sh:property [ sh:path pa:idempotentHint ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:boolean ] ;
 sh:property [ sh:path pa:openWorldHint ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:boolean ] ;
 sh:property [ sh:path fno:expects ; sh:minCount 1 ; sh:maxCount 1 ; sh:nodeKind sh:BlankNodeOrIRI ] ;
 sh:property [ sh:path fno:returns ; sh:minCount 1 ; sh:maxCount 1 ; sh:nodeKind sh:BlankNodeOrIRI ] .
pa:ExecutionShape a sh:NodeShape ; sh:targetClass pa:ExecutionProfile ;
 sh:property [ sh:path pa:confinementMode ; sh:minCount 1 ; sh:maxCount 1 ; sh:in ( "aa-exec" "change_hat" ) ] ;
 sh:property [ sh:path pa:profileName ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:hatName ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:egressAllowed ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:boolean ] ;
 sh:property [ sh:path pa:addressSpaceBytes ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:positiveInteger ] ;
 sh:property [ sh:path pa:fileSizeBytes ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:positiveInteger ] ;
 sh:property [ sh:path pa:processCount ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:positiveInteger ] ;
 sh:property [ sh:path pa:cpuSeconds ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:positiveInteger ] ;
 sh:property [ sh:path pa:openFileCount ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:positiveInteger ] ;
 sh:property [ sh:path pa:rule ; sh:nodeKind sh:BlankNodeOrIRI ] ;
 sh:property [ sh:path pa:subprofile ; sh:node pa:ExecutionShape ] ;
 sh:property [ sh:path pa:waiver ; sh:node pa:WaiverShape ] .
pa:WaiverShape a sh:NodeShape ; sh:targetClass pa:Waiver ;
 sh:property [ sh:path pa:waivedRule ; sh:minCount 1 ; sh:maxCount 1 ; sh:class pa:FileRule ] ;
 sh:property [ sh:path pa:reviewer ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ; sh:minLength 1 ] ;
 sh:property [ sh:path pa:rationale ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ; sh:minLength 1 ] ;
 sh:property [ sh:path pa:waiverSource ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:anyURI ] .
pa:ContextShape a sh:NodeShape ; sh:targetClass pa:Context ;
 sh:property [ sh:path pa:outputProjectionJq ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:maxOutputBytes ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:positiveInteger ] ;
 sh:property [ sh:path pa:historyRetentionTurns ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:nonNegativeInteger ] ;
 sh:property [ sh:path pa:intentTags ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:loadingStrategy ; sh:minCount 1 ; sh:maxCount 1 ; sh:in ( "always" "on_demand" ) ] .
pa:HintShape a sh:NodeShape ; sh:targetClass pa:Hint ;
 sh:property [ sh:path pa:maxEditDistance ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:nonNegativeInteger ] ;
 sh:property [ sh:path pa:alias ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:typeCoercion ; sh:minCount 1 ; sh:in ( "none" "string-to-integer" "integer-to-string" ) ] ;
 sh:property [ sh:path pa:caseFold ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:boolean ] ;
 sh:property [ sh:path pa:unicodeFold ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:boolean ] ;
 sh:property [ sh:path pa:taint ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:sessionBinding ; sh:maxCount 1 ; sh:nodeKind sh:IRI ] .
pa:NetworkShape a sh:NodeShape ; sh:targetClass pa:NetworkRule ;
 sh:property [ sh:path pa:domain ; sh:minCount 1 ; sh:maxCount 1 ; sh:in ( "inet" "inet6" ) ] ;
 sh:xone (
  [ sh:property [ sh:path pa:socketType ; sh:minCount 1 ; sh:maxCount 1 ] ; sh:property [ sh:path pa:protocol ; sh:maxCount 0 ] ]
  [ sh:property [ sh:path pa:protocol ; sh:minCount 1 ; sh:maxCount 1 ] ; sh:property [ sh:path pa:socketType ; sh:maxCount 0 ] ]
 ) ;
 sh:property [ sh:path pa:access ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:peerIp ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:peerPort ; sh:maxCount 1 ; sh:datatype xsd:string ] .
pa:FileShape a sh:NodeShape ; sh:targetClass pa:FileRule ;
 sh:property [ sh:path pa:pathGlob ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:filePermissions ; sh:maxCount 1 ; sh:datatype xsd:string ; sh:pattern "^[rwaklm]*$" ] ;
 sh:property [ sh:path pa:execTransition ; sh:maxCount 1 ; sh:in ( "ix" "px" "cx" "ux" "Ux" "Pix" "Cix" ) ] ;
 sh:property [ sh:path pa:transitionTarget ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:owner ; sh:maxCount 1 ; sh:datatype xsd:boolean ] .
pa:RuleShape a sh:NodeShape ; sh:targetClass pa:Rule ;
 sh:property [ sh:path pa:audit ; sh:maxCount 1 ; sh:datatype xsd:boolean ] ;
 sh:property [ sh:path pa:deny ; sh:maxCount 1 ; sh:datatype xsd:boolean ] .
pa:CapabilityShape a sh:NodeShape ; sh:targetClass pa:CapabilityRule ;
 sh:property [ sh:path pa:capability ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] .
pa:UnixShape a sh:NodeShape ; sh:targetClass pa:UnixRule ;
 sh:property [ sh:path pa:access ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:socketType ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:address ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:peerAddress ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:peerLabel ; sh:maxCount 1 ; sh:datatype xsd:string ] .
pa:SignalShape a sh:NodeShape ; sh:targetClass pa:SignalRule ;
 sh:property [ sh:path pa:access ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:signalSet ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:peerLabel ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] .
pa:PtraceShape a sh:NodeShape ; sh:targetClass pa:PtraceRule ;
 sh:property [ sh:path pa:access ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;
 sh:property [ sh:path pa:peerLabel ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] .
pa:MountShape a sh:NodeShape ; sh:targetClass pa:MountRule ;
 sh:property [ sh:path pa:mountOperation ; sh:minCount 1 ; sh:maxCount 1 ; sh:in ( "mount" "remount" "umount" "pivot_root" ) ] ;
 sh:property [ sh:path pa:deny ; sh:hasValue true ; sh:minCount 1 ; sh:maxCount 1 ] .
```

Semantic admission checks that FnO lists are finite and ordered, parameter
predicates uniquely match input properties, requiredness and datatypes agree,
all output bindings match the output shape, and every function has exactly one
mapping to an admitted host implementation. It rejects cyclic subprofiles, resolves every `pa:subtool` edge, computes
transitive subtool/abstraction security closure under a resource ceiling, and
rejects unbounded or unresolved nested-call references. It also rejects
undeclared variables, malformed globs, invalid access sets, conflicting write
and append permissions, invalid transitions or targets, network type/protocol
co-presence, malformed address/port expressions and any mount allow. Every
rule node is checked against both the common qualifier shape and its unique
concrete rule shape, regardless of explicit subclass typing. These checks
supplement SHACL rather than relying on RDFS inference to materialize types.

FnO reading uses an explicit checked subdataset view: the catalog document's
`rdf:type`, `dcterms:isPartOf`, `rdfs:label` and `rdfs:comment`; functions'
FnO properties, labels and admitted additional `rdf:type` values; parameter
and output FnO properties; implementation format and `rdfs:seeAlso`; blank
mapping nodes and their FnO/FnOM mapping fields; and the exact list-cell
closure. This partition is passed to `FnoCatalog::from_dataset` with ontology
`https://example.org/toolgraph#`. All other triples and reifiers stay in the
original dataset; shape/reifier projections consume that source. No unknown
FnO/FnOM predicate is filtered away: it remains in the FnO view and the strict
reader refuses it. Missing catalog fields are errors, never supplied defaults.
Every source row has exactly one partition owner or explicit shared-reference
role, so extracting a view cannot hide invalid security metadata. IRI mapping
nodes are refused: upstream requires native blank-node mapping identities.

Unconfined `ux` or `Ux` transitions require `pa:waiver` naming the exact file
rule, authenticated reviewer, nonempty rationale and immutable approval source.
The caller trust registry verifies the complete waiver bytes and scope; a
self-asserted graph waiver has no authority. Missing, stale or broader waivers
refuse admission. No worked tool declares an unconfined transition.

MCP annotation mapping is exact: `pa:title` ↔ `annotations.title` and each of
the four `*Hint` predicates ↔ the same-named boolean field. Duplicate JSON
keys, wrong types and unknown security fields are refused. Decoding preserves
absence separately from an explicit value; effective defaults follow MCP:
read-only false, destructive true, idempotent false, open-world true. Encoding
preserves original presence, while graph admission materializes and records
the effective values for security checking. `Effects::ReadOnly` requires both
a true hint and a statically read-only admitted implementation; every other
tool is `Effects::Mutating`. Hints never establish trust.

Reifier aliases become `ParameterSpec::alias`; `RepairPolicy::Structural` is
chosen when all inference permissions are zero/false/none, otherwise
`RepairPolicy::Tolerant` subject to the effect guard. Current core does not
have per-parameter edit-distance, coercion or folding switches. The bridge
must enforce the graph's bounds against itemized recovery evidence, refusing
an unrepresentable or uncheckable hint. It must never silently broaden them.
Lossless declaration adaptation also needs an explicit Phase 3 core extension:
`ToolSet::from_mcp_with_metadata_governed(json, metadata, governor)`, on the
existing MCP adapter assembly path. Each `DeclarationMetadata` binds the tool
name, checked `Effects`, `RepairPolicy`, and `ParameterAliases` with canonical
object-member paths. The adapter applies those records to private pending
parameters before normal compilation, retaining the exact schema and original
declaration text. Existing `from_mcp` uses the same parser with empty metadata
and continues to default to `Mutating`; graph annotations do not already change
that behavior. Unknown tool/path, duplicate records, alias collisions and
unmodeled hinted paths are typed definition/collision failures, and exhaustion
is a resource error. Reconstructing a weaker typed schema from the adapter's
projection is forbidden. This extension adds no dependency or ambient access
to core. The bridge's actual production caller must use this path.

Taint and session bindings become trusted Datalog facts and mandatory runtime
checks. They do not become AppArmor variables: only host-validated canonical
`WORKSPACE_ROOT`, `TOOL_TMP`, and imported tunables may enter policy expansion.
A model argument can never set a policy path or an authenticated fact.

## End-to-end example — specified

The database tool returns orders belonging to the authenticated customer in an
active tenant. The file tool edits a relative path under `--root` and requires
the host's existing version/lock/receipt rules. `query_customer_orders` is
read-only for recovery but has narrowly scoped database egress. `edit_file`
is mutating and has no egress. Both outputs have complete schemas; the field
mask is applied after validation, never used to conceal an invalid result.

The examples use trusted host bindings `WORKSPACE_ROOT=/srv/purrassist/workspace`,
`TOOL_TMP=/run/purrassist/session-17`, database address `10.0.0.7`, and session
user `customer-17`. These public example values are not environment discovery.
The host must ensure the temporary directory is private and owned by the
session, and bind immutable canonical directories without symlink traversal.

```turtle
@prefix pa: <https://example.org/toolgraph#> .
@prefix ex: <https://example.org/tools/> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
@prefix fno: <https://w3id.org/function/ontology#> .
@prefix fnom: <https://w3id.org/function/vocabulary/mapping#> .
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix dcterms: <http://purl.org/dc/terms/> .

ex:QueryArguments a rdfs:Class .
ex:EditArguments a rdfs:Class .
ex:QueryResult a rdfs:Class .
ex:EditResult a rdfs:Class .
ex:catalog a owl:Ontology ; dcterms:isPartOf <https://example.org/toolgraph#> ;
 rdfs:label "Worked tool catalog" ; rdfs:comment "Reviewed tool profile catalog for this example." .

ex:query_customer_orders a pa:Tool, fno:Function ; rdfs:label "query_customer_orders" ;
 pa:name "query_customer_orders" ; pa:description "Query customer orders." ;
 pa:title "Customer orders" ; pa:readOnlyHint true ; pa:destructiveHint false ;
 pa:idempotentHint true ; pa:openWorldHint false ;
 fno:expects ( ex:customerParam ex:limitParam ) ; fno:returns ( ex:ordersOutput ) ;
 pa:inputShape ex:QueryInput ; pa:outputShape ex:QueryOutput ;
 pa:executionProfile ex:DatabaseProfile ; pa:contextManagement ex:QueryContext ;
 pa:parameterConstraints """PREFIX pa: <https://example.org/toolgraph#>
PREFIX ex: <https://example.org/tools/>
RULE ex:queryPermit { ?call pa:permits ex:query_customer_orders }
WHERE {
 ?call pa:tool ex:query_customer_orders . ?call pa:session ?session .
 ?call pa:customerId ?user . ?session pa:userId ?user .
 ?session pa:tenant ?tenant . ?tenant pa:active true .
}
RULE ex:queryDeny { ?call pa:denies ex:query_customer_orders }
WHERE {
 ?call pa:tool ex:query_customer_orders .
 NOT { ?call pa:permits ex:query_customer_orders }
}""" .
ex:edit_file a pa:Tool, fno:Function ; rdfs:label "edit_file" ;
 pa:name "edit_file" ; pa:description "Edit a workspace file." ;
 pa:title "Edit file" ; pa:readOnlyHint false ; pa:destructiveHint true ;
 pa:idempotentHint false ; pa:openWorldHint false ;
 fno:expects ( ex:pathParam ex:textParam ) ; fno:returns ( ex:editOutput ) ;
 pa:inputShape ex:EditInput ; pa:outputShape ex:EditOutput ;
 pa:executionProfile ex:FileProfile ; pa:contextManagement ex:EditContext ;
 pa:parameterConstraints """PREFIX pa: <https://example.org/toolgraph#>
PREFIX ex: <https://example.org/tools/>
RULE ex:editPermit { ?call pa:permits ex:edit_file }
WHERE {
 ?call pa:tool ex:edit_file . ?call pa:session ?session .
 ?session pa:tenant ?tenant . ?tenant pa:active true .
}
RULE ex:editDeny { ?call pa:denies ex:edit_file }
WHERE { ?call pa:tool ex:edit_file . NOT { ?call pa:permits ex:edit_file } }""" .
ex:customerParam a fno:Parameter ; fno:predicate ex:customer_id ; fno:type xsd:string ; fno:required true .
ex:pathParam a fno:Parameter ; fno:predicate ex:path ; fno:type xsd:string ; fno:required true .
ex:textParam a fno:Parameter ; fno:predicate ex:text ; fno:type xsd:string ; fno:required true .
ex:limitParam a fno:Parameter ; fno:predicate ex:limit ; fno:type xsd:integer ; fno:required true .
ex:ordersOutput a fno:Output ; fno:predicate ex:orders ; fno:type xsd:string .
ex:editOutput a fno:Output ; fno:predicate ex:version ; fno:type xsd:string .
_:queryMapping a fno:Mapping ; rdfs:label "query implementation binding" ; fno:function ex:query_customer_orders ; fno:implementation ex:queryImplementation ; fno:parameterMapping _:customerMap, _:limitMap ; fno:returnMapping _:ordersMap .
ex:queryImplementation a fno:Implementation ; dcterms:format "application/vnd.purrassist.host-tool" ; rdfs:seeAlso <urn:purrassist:host:query_customer_orders> .
_:editMapping a fno:Mapping ; rdfs:label "edit implementation binding" ; fno:function ex:edit_file ; fno:implementation ex:editImplementation ; fno:parameterMapping _:pathMap, _:textMap ; fno:returnMapping _:versionMap .
ex:editImplementation a fno:Implementation ; dcterms:format "application/vnd.purrassist.host-tool" ; rdfs:seeAlso <urn:purrassist:host:edit_file> .
_:customerMap a fnom:PropertyParameterMapping ; rdfs:label "customer binding" ; fnom:functionParameter ex:customerParam ; fnom:implementationProperty "customer_id" .
_:limitMap a fnom:PropertyParameterMapping ; rdfs:label "limit binding" ; fnom:functionParameter ex:limitParam ; fnom:implementationProperty "limit" .
_:pathMap a fnom:PropertyParameterMapping ; rdfs:label "path binding" ; fnom:functionParameter ex:pathParam ; fnom:implementationProperty "path" .
_:textMap a fnom:PropertyParameterMapping ; rdfs:label "text binding" ; fnom:functionParameter ex:textParam ; fnom:implementationProperty "text" .
_:ordersMap a fnom:DefaultReturnMapping ; rdfs:label "orders binding" ; fnom:functionOutput ex:ordersOutput ; fnom:implementationProperty "orders" .
_:versionMap a fnom:DefaultReturnMapping ; rdfs:label "version binding" ; fnom:functionOutput ex:editOutput ; fnom:implementationProperty "version" .
ex:QueryInput a sh:NodeShape ; sh:targetClass ex:QueryArguments ; sh:closed true ;
 sh:property ex:customerIdShape, ex:limitShape .
ex:customerIdShape a sh:PropertyShape ; sh:path ex:customer_id ; sh:datatype xsd:string ; sh:pattern "^customer-[0-9]+$" ; sh:minCount 1 ; sh:maxCount 1 .
<< ex:customerIdShape sh:path ex:customer_id ~ ex:customerClaim >> .
ex:customerClaim a pa:Hint ; pa:maxEditDistance 0 ; pa:alias "customer" ;
 pa:typeCoercion "none" ; pa:caseFold false ; pa:unicodeFold false ;
 pa:taint "personal-data" ; pa:sessionBinding pa:userId .
ex:limitShape a sh:PropertyShape ; sh:path ex:limit ; sh:datatype xsd:integer ; sh:minInclusive 1 ; sh:maxInclusive 100 ; sh:minCount 1 ; sh:maxCount 1 .
<< ex:limitShape sh:path ex:limit ~ ex:limitClaim >> .
ex:limitClaim a pa:Hint ; pa:maxEditDistance 0 ; pa:typeCoercion "string-to-integer" ; pa:caseFold false ; pa:unicodeFold false .
ex:EditInput a sh:NodeShape ; sh:targetClass ex:EditArguments ; sh:closed true ; sh:property ex:pathShape, ex:textShape .
ex:pathShape a sh:PropertyShape ; sh:path ex:path ; sh:datatype xsd:string ; sh:minCount 1 ; sh:maxCount 1 .
ex:textShape a sh:PropertyShape ; sh:path ex:text ; sh:datatype xsd:string ; sh:minCount 1 ; sh:maxCount 1 .
<< ex:pathShape sh:path ex:path ~ ex:pathClaim >> .
ex:pathClaim a pa:Hint ; pa:maxEditDistance 0 ; pa:typeCoercion "none" ;
 pa:caseFold false ; pa:unicodeFold false ; pa:taint "untrusted-path" .
ex:QueryOutput a sh:NodeShape ; sh:targetClass ex:QueryResult ; sh:closed true ;
 sh:property [ sh:path ex:orders ; sh:datatype xsd:string ; sh:minCount 1 ; sh:maxCount 1 ] .
ex:EditOutput a sh:NodeShape ; sh:targetClass ex:EditResult ; sh:closed true ;
 sh:property [ sh:path ex:version ; sh:datatype xsd:string ; sh:minCount 1 ; sh:maxCount 1 ] .
ex:QueryContext a pa:Context ; pa:outputProjectionJq ".orders" ; pa:maxOutputBytes 4096 ;
 pa:historyRetentionTurns 2 ; pa:intentTags "customer", "orders" ; pa:loadingStrategy "on_demand" .
ex:EditContext a pa:Context ; pa:outputProjectionJq ".version" ; pa:maxOutputBytes 1024 ;
 pa:historyRetentionTurns 1 ; pa:intentTags "edit", "file" ; pa:loadingStrategy "always" .
ex:DatabaseProfile a pa:ExecutionProfile ; pa:confinementMode "change_hat" ;
 pa:profileName "purrassist.tool.query_customer_orders" ; pa:hatName "query_customer_orders" ;
 pa:egressAllowed true ; pa:addressSpaceBytes 268435456 ; pa:fileSizeBytes 1048576 ;
 pa:processCount 16 ; pa:cpuSeconds 5 ; pa:openFileCount 64 ;
 pa:rule ex:DatabaseNetwork, ex:TemporaryRead, ex:SshDeny, ex:EnvDeny,
 ex:SignalRule, ex:PtraceRule, ex:UnixRule, ex:MountDeny, ex:RemountDeny, ex:UmountDeny, ex:PivotDeny .
ex:FileProfile a pa:ExecutionProfile ; pa:confinementMode "aa-exec" ;
 pa:profileName "purrassist.tool.edit_file" ; pa:hatName "edit_file" ;
 pa:egressAllowed false ; pa:addressSpaceBytes 268435456 ; pa:fileSizeBytes 1048576 ;
 pa:processCount 16 ; pa:cpuSeconds 5 ; pa:openFileCount 64 ;
 pa:rule ex:WorkspaceFile, ex:TemporaryFile, ex:SshDeny, ex:EnvDeny,
 ex:SignalRule, ex:PtraceRule, ex:UnixRule, ex:MountDeny, ex:RemountDeny, ex:UmountDeny, ex:PivotDeny .
ex:WorkspaceFile a pa:FileRule ; pa:pathGlob "@{WORKSPACE_ROOT}/**" ; pa:filePermissions "rwkl" ; pa:owner true .
ex:TemporaryRead a pa:FileRule ; pa:pathGlob "@{TOOL_TMP}/**" ; pa:filePermissions "r" ; pa:owner true .
ex:TemporaryFile a pa:FileRule ; pa:pathGlob "@{TOOL_TMP}/**" ; pa:filePermissions "rwk" ; pa:owner true .
ex:SshDeny a pa:FileRule ; pa:pathGlob "@{HOME}/.ssh/**" ; pa:filePermissions "rw" ; pa:execTransition "ix" ; pa:deny true ; pa:audit true .
ex:EnvDeny a pa:FileRule ; pa:pathGlob "/**/.env*" ; pa:filePermissions "rw" ; pa:execTransition "ix" ; pa:deny true ; pa:audit true .
ex:DatabaseNetwork a pa:NetworkRule ; pa:domain "inet" ; pa:socketType "stream" ; pa:access "connect" ; pa:peerIp "10.0.0.7" ; pa:peerPort "5432" .
ex:UnixRule a pa:UnixRule ; pa:access "send,receive" ; pa:socketType "stream" ; pa:address "@@{TOOL_TMP}/ipc" ; pa:peerLabel "purrassist.tools" ; pa:peerAddress "@@{TOOL_TMP}/host" .
ex:SignalRule a pa:SignalRule ; pa:access "receive" ; pa:signalSet "term,int" ; pa:peerLabel "purrassist.tools" .
ex:PtraceRule a pa:PtraceRule ; pa:access "trace,read,readby,tracedby" ; pa:peerLabel "**" ; pa:deny true .
ex:MountDeny a pa:MountRule ; pa:mountOperation "mount" ; pa:deny true .
ex:RemountDeny a pa:MountRule ; pa:mountOperation "remount" ; pa:deny true .
ex:UmountDeny a pa:MountRule ; pa:mountOperation "umount" ; pa:deny true .
ex:PivotDeny a pa:MountRule ; pa:mountOperation "pivot_root" ; pa:deny true .
```

The two policy literals above are repeated below as standalone SRL documents
for extraction and check-only validation. They are complete policy documents,
not references resolved by a network or by a permissive fallback.

```srl
PREFIX pa: <https://example.org/toolgraph#>
PREFIX ex: <https://example.org/tools/>
RULE ex:queryPermit { ?call pa:permits ex:query_customer_orders }
WHERE {
 ?call pa:tool ex:query_customer_orders . ?call pa:session ?session .
 ?call pa:customerId ?user . ?session pa:userId ?user .
 ?session pa:tenant ?tenant . ?tenant pa:active true .
}
RULE ex:queryDeny { ?call pa:denies ex:query_customer_orders }
WHERE {
 ?call pa:tool ex:query_customer_orders .
 NOT { ?call pa:permits ex:query_customer_orders }
}
```

```srl
PREFIX pa: <https://example.org/toolgraph#>
PREFIX ex: <https://example.org/tools/>
RULE ex:editPermit { ?call pa:permits ex:edit_file }
WHERE {
 ?call pa:tool ex:edit_file . ?call pa:session ?session .
 ?session pa:tenant ?tenant . ?tenant pa:active true .
}
RULE ex:editDeny { ?call pa:denies ex:edit_file }
WHERE { ?call pa:tool ex:edit_file . NOT { ?call pa:permits ex:edit_file } }
```

Policy provenance is part of admission. The caller supplies an immutable,
host-owned `TrustedPolicyRegistry`; each entry binds an exact SRL document
digest, allowed tool ids, vocabulary identity, policy issuer and trusted
session-fact schema. The worked `parameterConstraints` documents above are
trusted gate policies only after this exact binding succeeds. They may derive
`pa:permits`, `pa:denies` and `pa:conflict`. An absent registry entry, changed
document byte or untrusted issuer is `UntrustedPolicy`, before evaluation.
A graph does not make its own policy trusted by asserting a registry triple.

Separately admitted tool-authored rules may derive only the exact intermediate
predicate set `pa:candidateAllowed`, `pa:candidateDenied`, with subjects from the
current call facts and objects from the admitted tool table. The host applies
`NeverDeriveDeclarations` plus an explicit protected-head guard to their
programs, excluding decision, authenticated-session, vocabulary and executable
binding predicates from derivation. Trusted gate programs consume those
intermediates explicitly; there is no implicit conversion to permits. The two
worked policies need no untrusted intermediate program. Their heads are legal
because their rule bytes are in the trusted caller registry, not because they
were loaded from a tool graph. Every inference receipt identifies the rule
stage and trusted registry entry.

The gate reads exactly the vocabulary-bound predicates `pa:permits`,
`pa:denies`, and `pa:conflict`. It permits only a derived `(call, permits,
resolved-tool)` with no matching deny and no conflict in the admitted snapshot.
No permit, multiple tool targets, a stale session, or incomplete inference is a
refusal. Proofs bind the canonical argument digest, graph digest, session-fact
digest, rule contract hash and consumed resources. Facts supplied by the model
cannot assert any of these decision predicates. Static conflict rules reject
read-only tools granting `w`, `a`, `k`, `l` or mount operations, and
`egressAllowed false` profiles granting `network inet` or `inet6`. Glob
intersection and grant/deny analysis must use the parsed AppArmor language;
string-prefix comparisons do not establish safety.

### AppArmor output — specified

The following is one complete generated file, `/etc/apparmor.d/purrassist.tools`.
It includes a parent with both hats and both standalone profiles so the caller
can choose either supported execution mode without regenerating permissions.
A deny execution rule renders `x` rather than `ix`; the transition on a denied
path is not an executable grant. The query worker has only read access to its session temporary directory;
its read-only contract includes that grant. No profiles are loaded by documentation
validation. Offline qualification uses AppArmor 4.1.7 with the worked
policy's explicit `abi <abi/4.0-ip>,` requirement and an explicit matching target
feature file. Every warning is enabled and fatal:

```sh
apparmor_parser -QK --kernel-features /etc/apparmor.d/abi/4.0-ip \
  --warn=all --Werror example.apparmor
```

The selected feature file describes the offline compilation target; it does not
attest to the running kernel or loaded policy. Record its SHA-256 and packaged
provenance beside the parser identity. Default parser success can otherwise
warn that network rules are not enforced and omit their permissions entirely.

```apparmor
abi <abi/4.0-ip>,
include <tunables/global>
@{WORKSPACE_ROOT}=/srv/purrassist/workspace
@{TOOL_TMP}=/run/purrassist/session-17
profile purrassist.tools flags=(attach_disconnected) {
  ^query_customer_orders {
    owner @{TOOL_TMP}/** r,
    audit deny @{HOME}/.ssh/** rwx,
    audit deny /**/.env* rwx,
    deny capability sys_admin,
    deny capability sys_ptrace,
    deny capability sys_module,
    deny capability sys_rawio,
    deny capability net_admin,
    deny capability dac_override,
    deny capability dac_read_search,
    deny capability setuid,
    deny capability setgid,
    deny capability mknod,
    deny capability chown,
    deny capability fowner,
    deny capability kill,
    deny capability net_bind_service,
    network (connect) inet stream peer=(ip=10.0.0.7 port=5432),
    unix (send, receive) type=stream addr="@@{TOOL_TMP}/ipc" peer=(label=purrassist.tools addr="@@{TOOL_TMP}/host"),
    signal (receive) set=(term, int) peer=purrassist.tools,
    deny ptrace (trace, read, readby, tracedby) peer=**,
    deny mount,
    deny remount,
    deny umount,
    deny pivot_root,
    set rlimit as <= 268435456,
    set rlimit fsize <= 1048576,
    set rlimit nproc <= 16,
    set rlimit cpu <= 5 seconds,
    set rlimit nofile <= 64,
  }
  ^edit_file {
    owner @{WORKSPACE_ROOT}/** rwkl,
    owner @{TOOL_TMP}/** rwk,
    audit deny @{HOME}/.ssh/** rwx,
    audit deny /**/.env* rwx,
    deny capability sys_admin,
    deny capability sys_ptrace,
    deny capability sys_module,
    deny capability sys_rawio,
    deny capability net_admin,
    deny capability dac_override,
    deny capability dac_read_search,
    deny capability setuid,
    deny capability setgid,
    deny capability mknod,
    deny capability chown,
    deny capability fowner,
    deny capability kill,
    deny capability net_bind_service,
    unix (send, receive) type=stream addr="@@{TOOL_TMP}/ipc" peer=(label=purrassist.tools addr="@@{TOOL_TMP}/host"),
    signal (receive) set=(term, int) peer=purrassist.tools,
    deny ptrace (trace, read, readby, tracedby) peer=**,
    deny mount,
    deny remount,
    deny umount,
    deny pivot_root,
    set rlimit as <= 268435456,
    set rlimit fsize <= 1048576,
    set rlimit nproc <= 16,
    set rlimit cpu <= 5 seconds,
    set rlimit nofile <= 64,
  }
}
profile purrassist.tool.query_customer_orders flags=(attach_disconnected) {
  owner @{TOOL_TMP}/** r,
  audit deny @{HOME}/.ssh/** rwx,
  audit deny /**/.env* rwx,
  deny capability sys_admin,
  deny capability sys_ptrace,
  deny capability sys_module,
  deny capability sys_rawio,
  deny capability net_admin,
  deny capability dac_override,
  deny capability dac_read_search,
  deny capability setuid,
  deny capability setgid,
  deny capability mknod,
  deny capability chown,
  deny capability fowner,
  deny capability kill,
  deny capability net_bind_service,
  network (connect) inet stream peer=(ip=10.0.0.7 port=5432),
  unix (send, receive) type=stream addr="@@{TOOL_TMP}/ipc" peer=(label=purrassist.tools addr="@@{TOOL_TMP}/host"),
  signal (receive) set=(term, int) peer=purrassist.tools,
  deny ptrace (trace, read, readby, tracedby) peer=**,
  deny mount,
  deny remount,
  deny umount,
  deny pivot_root,
  set rlimit as <= 268435456,
  set rlimit fsize <= 1048576,
  set rlimit nproc <= 16,
  set rlimit cpu <= 5 seconds,
  set rlimit nofile <= 64,
}
profile purrassist.tool.edit_file flags=(attach_disconnected) {
  owner @{WORKSPACE_ROOT}/** rwkl,
  owner @{TOOL_TMP}/** rwk,
  audit deny @{HOME}/.ssh/** rwx,
  audit deny /**/.env* rwx,
  deny capability sys_admin,
  deny capability sys_ptrace,
  deny capability sys_module,
  deny capability sys_rawio,
  deny capability net_admin,
  deny capability dac_override,
  deny capability dac_read_search,
  deny capability setuid,
  deny capability setgid,
  deny capability mknod,
  deny capability chown,
  deny capability fowner,
  deny capability kill,
  deny capability net_bind_service,
  unix (send, receive) type=stream addr="@@{TOOL_TMP}/ipc" peer=(label=purrassist.tools addr="@@{TOOL_TMP}/host"),
  signal (receive) set=(term, int) peer=purrassist.tools,
  deny ptrace (trace, read, readby, tracedby) peer=**,
  deny mount,
  deny remount,
  deny umount,
  deny pivot_root,
  set rlimit as <= 268435456,
  set rlimit fsize <= 1048576,
  set rlimit nproc <= 16,
  set rlimit cpu <= 5 seconds,
  set rlimit nofile <= 64,
}
```

Network access precedes the domain, followed by exactly one socket type or
protocol, then local `ip=` / `port=` and optional `peer=(ip= port=)` conditions.
The concrete address family must agree with the declared `inet` or `inet6`
domain; reject mismatches even when the parser accepts their syntax. Peer
conditions cannot accompany local-only `create`, `bind`, `listen`, `shutdown`,
`getattr`, `setattr`, `getopt` or `setopt` accesses. There is no CIDR form in this
grammar, and network type and protocol are exclusive. Changing the database
destination to `10.0.0.0/8` returns
`UnenforceableNetworkScope` with that exact source node and destination.
It produces no policy. Explicit host enumeration is allowed only under a
caller-set address-count ceiling and only if the enumerated set is exact;
exhaustion is `SynthesisError::Exhausted`, never a widened network grant. The
worked scoped-IP policy requires the fine-grained `network_v8/af_inet` and
`network/af_unix` mediation represented by the selected `4.0-ip` ABI. The host must verify these
features and the exact loaded policy before dispatch; unsupported mediation is
a hard refusal, never a coarse network fallback. Other v3/v4 target policies
retain their original acceptance; a target that cannot enforce a required scope
is refused. A generated policy must also fit the running kernel's mediation
features; parser acceptance alone does not prove network enforcement or that a worker can start. Host
qualification must prove a usable least-privilege startup closure for the
actual admitted executable. These examples deliberately grant no unspecified
loader, executable or inherited file-descriptor rights.

The Unix rules name abstract sockets, not filesystem sockets. The first `@`
in `@@{TOOL_TMP}/ipc` is the literal abstract-address marker; the second starts
the trusted path variable. With the worked binding the address is
`@/run/purrassist/session-17/ipc`, and the peer address ends in `/host`.
Using only `@{TOOL_TMP}/ipc` is incorrect: after expansion the parser strips the
first slash while forming an abstract address, targeting `@run/.../ipc` instead.
Validate address bindings after expansion and retain the literal abstract marker.
Filesystem Unix sockets require file-access rules; a filesystem path must not be
silently emitted as an abstract address. Peer labels remain explicit and trusted.

Offline acceptance must inspect compiled network/DFA evidence for the exact
`10.0.0.7:5432` peer and the intended abstract socket names. Compare neighboring
wrong IP/port or address spellings and a default-feature compilation that drops
network mediation. Neither fatal-warning parsing nor this target-feature proof
establishes actual kernel enforcement. The eligible loaded-profile smoke tests
and complete graph/synthesis/host acceptance remain required under #34/#35.

`change_hat` changes the calling thread's confinement; it is permitted only in
an isolated worker with no cross-session shared execution, and with a host-owned
unguessable return token. Entering or restoring a hat must verify the resulting
label. The pure graph crate accepts the token as opaque host input and performs
no random, filesystem or process access. `aa-exec` must select the exact profile,
refuse an unconfined result, close unadmitted descriptors and return typed launch,
label or mediation failures. Neither mode can silently fall back to the other.

**G7 compiler prerequisite delivered in #70 and #43; loader integration remains open:**
upstream PurRDF issue #788 closed through PR #806, and numeric prerequisite #885
closed through PR #888, adopted at exact revision
`742bbbfbe2240e1fc6f178badd049420753ef358`. The public
`purrdf_shapes::json_schema::compile_tool_arguments` accepts caller-owned
root-shape/class identities and parent-scoped FnO predicate/member bindings.
It uses the shared constraint compiler for bare string/integer/number/boolean,
object and array schemas, requiredness, cardinality and closedness. Decimal
constraints retain arbitrary precision. Finite IEEE constants use exact decimal
values; unrestricted float/double domains record the source-located
`plain:ieee-representability` obligation for original SHACL checking. Finite
admitted enums or scalar constants discharge it. Logical constraints and
reference caches preserve residual exactness boundaries. Unsupported RDF
representations, invalid bindings and identity-losing numeric constants return
typed errors with the original predicate and bound member. Complete losses keep
their original source; original SHACL checking remains mandatory where the
projection loses semantics. The original `compile` API retains JSON-LD output.

The four worked input/output schemas below are direct downstream acceptance
fixtures for that adopted API, exercised by the `plain_tool_schemas` example
with exact schema comparison and the existing first-party validator. The native
QueryInput regular expression, after JSON string decoding, is
`^customer\u{2d}(?:[0-9])+$`.
The manifest/declaration bytes below remain specified #33 acceptance, including
its production graph loading and declaration emission. These consumer fixtures
do not establish those behaviors. #33 retains
its full loader/codec, original source, FnO mapping, SHACL/policy checking,
facade/host and packaging contract. Local reimplementation, post-hoc JSON-LD
schema surgery and reconstruction through core's partial typed schema model
remain forbidden. Native logical work/bytes/nodes limits and their explicit
accounting/cancellation limits are documented in [the G7 plan](TOOLS_PLAN.md#g7-upstream-plain-tool-schema-projection-prerequisite).

### MCP Extended Tool Manifest — specified

The normalized declarations below are derived from the worked graph. Standard
MCP consumers see `name`, `description`, `inputSchema`, `outputSchema` and
`annotations`. The graph-aware host also requires `_meta.security`,
`_meta.executionProfile` and `_meta.contextManagement`. Tool-local
`graphBinding` identifies a tool in the enclosing manifest's immutable RDF
bundle. The bundle contains the complete canonical RDF dataset, prefix table,
SRL documents, admitted abstraction contents and loss-profile identity; no
unresolved remote reference is accepted. A wire manifest without the required
bundle cannot be admitted. Unknown RDF triples and reifiers survive in this
bundle even when a projection cannot interpret them; unknown security semantics
still refuse admission. A plain MCP consumer's metadata ignorance is a
projection loss and never an execution capability.

The current `codec::mcp::ExecutionProfile` fragment reader validates the complete
JSON execution profile while retaining its original nodes and locations. Both
`aa-exec` and `change_hat` are explicit; no mode is inferred. Its five resource
quantities must be mathematically positive integers representable as `u64`;
zero, negatives, fractions and larger values receive typed refusals. Integral
exponent spellings are accepted without rounding. These wire limits are distinct
from a selected AppArmor renderer's representability limits. Profile and hat
strings are preserved without extra syntax restrictions. Fragment encoding fixes
field order while preserving numeric spellings and decoded string values through the shared JSON writer.
This is wire validation; RDF agreement and full graph/manifest conversion remain
#33 acceptance.

The JSON example uses a top-level `_meta.security.sourceGraph` Turtle string,
added in the complete manifest block below, as the self-contained bundle's
source dataset. The typed metadata and dataset must agree exactly; disagreement
is `InvalidManifest`, not a choice between competing authorities. Encoding
preserves the original dataset and declaration bytes; deterministic compilation
uses canonical semantic identities. The bundle's digest uses PurRDF GTS BLAKE3,
while the existing receipt v3 remains SHA-256 and binds that bundle digest.
Optional COSE_Sign1 is a distinct authenticity layer, never a replacement for
shape, policy or confinement checks.

```json
{
  "tools": [
    {
      "name": "edit_file",
      "description": "Edit a workspace file.",
      "inputSchema": {
        "type": "object",
        "properties": {
          "path": {
            "type": "string"
          },
          "text": {
            "type": "string"
          }
        },
        "required": [
          "path",
          "text"
        ],
        "additionalProperties": false
      },
      "outputSchema": {
        "type": "object",
        "properties": {
          "version": {
            "type": "string"
          }
        },
        "required": [
          "version"
        ],
        "additionalProperties": false
      },
      "annotations": {
        "title": "Edit file",
        "readOnlyHint": false,
        "destructiveHint": true,
        "idempotentHint": false,
        "openWorldHint": false
      },
      "_meta": {
        "security": {
          "graphBinding": "https://example.org/tools/edit_file",
          "parameterConstraints": "PREFIX pa: <https://example.org/toolgraph#>\nPREFIX ex: <https://example.org/tools/>\nRULE ex:editPermit { ?call pa:permits ex:edit_file }\nWHERE {\n ?call pa:tool ex:edit_file . ?call pa:session ?session .\n ?session pa:tenant ?tenant . ?tenant pa:active true .\n}\nRULE ex:editDeny { ?call pa:denies ex:edit_file }\nWHERE { ?call pa:tool ex:edit_file . NOT { ?call pa:permits ex:edit_file } }",
          "subtools": [],
          "recoveryHints": [
            "https://example.org/tools/pathClaim"
          ],
          "policyTrust": "caller-registry-required"
        },
        "executionProfile": {
          "graphBinding": "https://example.org/tools/FileProfile",
          "mode": "aa-exec",
          "profileName": "purrassist.tool.edit_file",
          "hatName": "edit_file",
          "egressAllowed": false,
          "addressSpaceBytes": 268435456,
          "fileSizeBytes": 1048576,
          "processCount": 16,
          "cpuSeconds": 5,
          "openFileCount": 64
        },
        "contextManagement": {
          "outputProjectionJq": ".version",
          "maxOutputBytes": 1024,
          "outputEnforcement": "exact-bytes",
          "historyRetentionTurns": 1,
          "intentTags": [
            "edit",
            "file"
          ],
          "loadingStrategy": "always"
        }
      }
    },
    {
      "name": "query_customer_orders",
      "description": "Query customer orders.",
      "inputSchema": {
        "type": "object",
        "properties": {
          "customer_id": {
            "type": "string",
            "pattern": "^customer-[0-9]+$"
          },
          "limit": {
            "type": "integer",
            "minimum": 1,
            "maximum": 100
          }
        },
        "required": [
          "customer_id",
          "limit"
        ],
        "additionalProperties": false
      },
      "outputSchema": {
        "type": "object",
        "properties": {
          "orders": {
            "type": "string"
          }
        },
        "required": [
          "orders"
        ],
        "additionalProperties": false
      },
      "annotations": {
        "title": "Customer orders",
        "readOnlyHint": true,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": false
      },
      "_meta": {
        "security": {
          "graphBinding": "https://example.org/tools/query_customer_orders",
          "parameterConstraints": "PREFIX pa: <https://example.org/toolgraph#>\nPREFIX ex: <https://example.org/tools/>\nRULE ex:queryPermit { ?call pa:permits ex:query_customer_orders }\nWHERE {\n ?call pa:tool ex:query_customer_orders . ?call pa:session ?session .\n ?call pa:customerId ?user . ?session pa:userId ?user .\n ?session pa:tenant ?tenant . ?tenant pa:active true .\n}\nRULE ex:queryDeny { ?call pa:denies ex:query_customer_orders }\nWHERE {\n ?call pa:tool ex:query_customer_orders .\n NOT { ?call pa:permits ex:query_customer_orders }\n}",
          "subtools": [],
          "recoveryHints": [
            "https://example.org/tools/customerClaim",
            "https://example.org/tools/limitClaim"
          ],
          "policyTrust": "caller-registry-required"
        },
        "executionProfile": {
          "graphBinding": "https://example.org/tools/DatabaseProfile",
          "mode": "change_hat",
          "profileName": "purrassist.tool.query_customer_orders",
          "hatName": "query_customer_orders",
          "egressAllowed": true,
          "addressSpaceBytes": 268435456,
          "fileSizeBytes": 1048576,
          "processCount": 16,
          "cpuSeconds": 5,
          "openFileCount": 64
        },
        "contextManagement": {
          "outputProjectionJq": ".orders",
          "maxOutputBytes": 4096,
          "outputEnforcement": "exact-bytes",
          "historyRetentionTurns": 2,
          "intentTags": [
            "customer",
            "orders"
          ],
          "loadingStrategy": "on_demand"
        }
      }
    }
  ],
  "_meta": {
    "security": {
      "format": "purrassist-toolgraph-v2",
      "mediaType": "text/turtle",
      "prefixes": {
        "pa": "https://example.org/toolgraph#",
        "ex": "https://example.org/tools/"
      },
      "sourceGraph": "@prefix pa: <https://example.org/toolgraph#> .\n@prefix ex: <https://example.org/tools/> .\n@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .\n@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n@prefix owl: <http://www.w3.org/2002/07/owl#> .\n@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n@prefix sh: <http://www.w3.org/ns/shacl#> .\n@prefix fno: <https://w3id.org/function/ontology#> .\n@prefix fnom: <https://w3id.org/function/vocabulary/mapping#> .\n\npa:Node a rdfs:Class ; rdfs:comment \"A tool-graph vocabulary node.\" .\npa:Tool a rdfs:Class ; rdfs:subClassOf fno:Function, pa:Node ; rdfs:comment \"A callable FnO function with security and context bindings.\" .\npa:ExecutionProfile a rdfs:Class ; rdfs:subClassOf pa:Node ; rdfs:comment \"One complete confinement contract.\" .\npa:Context a rdfs:Class ; rdfs:subClassOf pa:Node ; rdfs:comment \"Output projection and prompt loading contract.\" .\npa:Hint a rdfs:Class ; rdfs:subClassOf pa:Node ; rdfs:comment \"RDF 1.2 reifier annotating a parameter property shape's sh:path triple.\" .\npa:Rule a rdfs:Class ; rdfs:subClassOf pa:Node ; rdfs:comment \"One typed AppArmor rule; omitted permissions are not granted.\" .\npa:FileRule a rdfs:Class ; rdfs:subClassOf pa:Rule ; rdfs:comment \"AppArmor pathname glob and access or execution transition.\" .\npa:CapabilityRule a rdfs:Class ; rdfs:subClassOf pa:Rule ; rdfs:comment \"One named Linux capability allow or deny.\" .\npa:NetworkRule a rdfs:Class ; rdfs:subClassOf pa:Rule ; rdfs:comment \"AppArmor 4.1 network rule with exact address scope.\" .\npa:UnixRule a rdfs:Class ; rdfs:subClassOf pa:Rule ; rdfs:comment \"Unix socket access and peer mediation.\" .\npa:SignalRule a rdfs:Class ; rdfs:subClassOf pa:Rule ; rdfs:comment \"Signal access, signal set and peer label.\" .\npa:PtraceRule a rdfs:Class ; rdfs:subClassOf pa:Rule ; rdfs:comment \"Ptrace access and peer label.\" .\npa:MountRule a rdfs:Class ; rdfs:subClassOf pa:Rule ; rdfs:comment \"A denied mount, remount, umount or pivot_root operation.\" .\npa:Waiver a rdfs:Class ; rdfs:subClassOf pa:Node ; rdfs:comment \"A host-authenticated narrowly scoped exception for one unconfined exec transition.\" .\npa:Call a rdfs:Class ; rdfs:subClassOf pa:Node ; rdfs:comment \"A request-scoped immutable call identity.\" .\npa:Session a rdfs:Class ; rdfs:subClassOf pa:Node ; rdfs:comment \"Authenticated facts supplied by the trusted host.\" .\nfnom:Mapping a rdfs:Class ; owl:equivalentClass fno:Mapping ; rdfs:comment \"Caller alias normalized to the upstream FnO Mapping class.\" .\n\npa:name a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range xsd:string ; rdfs:comment \"Unique core tool name, independent of display title.\" .\npa:description a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range xsd:string ; rdfs:comment \"Model-visible description.\" .\npa:inputShape a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range sh:NodeShape ; rdfs:comment \"Complete argument SHACL shape.\" .\npa:outputShape a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range sh:NodeShape ; rdfs:comment \"Complete result SHACL shape.\" .\npa:title a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range xsd:string ; rdfs:comment \"MCP annotations.title, verbatim.\" .\npa:readOnlyHint a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range xsd:boolean ; rdfs:comment \"MCP annotations.readOnlyHint, a hint checked against actual declared effects.\" .\npa:destructiveHint a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range xsd:boolean ; rdfs:comment \"MCP annotations.destructiveHint, verbatim.\" .\npa:idempotentHint a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range xsd:boolean ; rdfs:comment \"MCP annotations.idempotentHint, verbatim.\" .\npa:openWorldHint a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range xsd:boolean ; rdfs:comment \"MCP annotations.openWorldHint, verbatim; not a permission.\" .\npa:waiver a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range pa:Waiver ; rdfs:comment \"Audited transition waiver honored only when the host registry matches its source and exact rule.\" .\npa:waivedRule a rdf:Property ; rdfs:domain pa:Waiver ; rdfs:range pa:FileRule ; rdfs:comment \"Exactly one unconfined exec rule covered by this waiver.\" .\npa:reviewer a rdf:Property ; rdfs:domain pa:Waiver ; rdfs:range xsd:string ; rdfs:comment \"Authenticated reviewer verified by caller trust registry.\" .\npa:rationale a rdf:Property ; rdfs:domain pa:Waiver ; rdfs:range xsd:string ; rdfs:comment \"Nonempty explicit security exception rationale.\" .\npa:waiverSource a rdf:Property ; rdfs:domain pa:Waiver ; rdfs:range xsd:anyURI ; rdfs:comment \"Immutable review source bound by host approval record.\" .\npa:subtool a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range pa:Tool ; rdfs:comment \"An explicit potential nested call edge; included transitively in static security closure and independently authorized at runtime.\" .\npa:executionProfile a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range pa:ExecutionProfile ; rdfs:comment \"Mandatory confinement contract.\" .\npa:contextManagement a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range pa:Context ; rdfs:comment \"Mandatory context management contract.\" .\npa:parameterConstraints a rdf:Property ; rdfs:domain pa:Tool ; rdfs:range xsd:string ; rdfs:comment \"Complete SPARQL 1.2 RL document, never executable host code.\" .\npa:maxEditDistance a rdf:Property ; rdfs:domain pa:Hint ; rdfs:range xsd:nonNegativeInteger ; rdfs:comment \"Maximum admitted name or value edit-distance inference; zero forbids it.\" .\npa:alias a rdf:Property ; rdfs:domain pa:Hint ; rdfs:range xsd:string ; rdfs:comment \"Explicit parameter-name alias projected to ParameterSpec::alias.\" .\npa:typeCoercion a rdf:Property ; rdfs:domain pa:Hint ; rdfs:range xsd:string ; rdfs:comment \"Allowed conversion name: string-to-integer, integer-to-string or none.\" .\npa:caseFold a rdf:Property ; rdfs:domain pa:Hint ; rdfs:range xsd:boolean ; rdfs:comment \"Permission for labeled case-folding inferences, never validation normalization.\" .\npa:unicodeFold a rdf:Property ; rdfs:domain pa:Hint ; rdfs:range xsd:boolean ; rdfs:comment \"Permission for labeled Unicode-folding inferences.\" .\npa:taint a rdf:Property ; rdfs:domain pa:Hint ; rdfs:range xsd:string ; rdfs:comment \"A label copied to the candidate's policy facts; not model-controlled.\" .\npa:sessionBinding a rdf:Property ; rdfs:domain pa:Hint ; rdfs:range rdf:Property ; rdfs:comment \"Trusted session predicate to which the argument must be equal.\" .\npa:confinementMode a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range xsd:string ; rdfs:comment \"aa-exec or change_hat; no unenforced mode.\" .\npa:profileName a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range xsd:string ; rdfs:comment \"Exact standalone AppArmor profile name.\" .\npa:hatName a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range xsd:string ; rdfs:comment \"Exact hat inside purrassist.tools.\" .\npa:egressAllowed a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range xsd:boolean ; rdfs:comment \"Whether the profile may grant outbound network access.\" .\npa:rule a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range pa:Rule ; rdfs:comment \"A granular AppArmor rule; order is canonicalized.\" .\npa:addressSpaceBytes a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range xsd:positiveInteger ; rdfs:comment \"set rlimit as in exact bytes.\" .\npa:fileSizeBytes a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range xsd:positiveInteger ; rdfs:comment \"set rlimit fsize in exact bytes.\" .\npa:processCount a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range xsd:positiveInteger ; rdfs:comment \"set rlimit nproc count.\" .\npa:cpuSeconds a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range xsd:positiveInteger ; rdfs:comment \"set rlimit cpu in whole seconds, never fractional seconds.\" .\npa:openFileCount a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range xsd:positiveInteger ; rdfs:comment \"set rlimit nofile count.\" .\npa:subprofile a rdf:Property ; rdfs:domain pa:ExecutionProfile ; rdfs:range pa:ExecutionProfile ; rdfs:comment \"A named child profile for a cx or Cix transition.\" .\npa:pathGlob a rdf:Property ; rdfs:domain pa:FileRule ; rdfs:range xsd:string ; rdfs:comment \"First-party parsed AppArmor glob including brace alternatives and declared @{VAR}.\" .\npa:filePermissions a rdf:Property ; rdfs:domain pa:FileRule ; rdfs:range xsd:string ; rdfs:comment \"Unique letters from r w a k l m; a and w are mutually exclusive.\" .\npa:execTransition a rdf:Property ; rdfs:domain pa:FileRule ; rdfs:range xsd:string ; rdfs:comment \"Exactly one of ix px cx ux Ux Pix Cix; unsafe ux requires explicit admission.\" .\npa:transitionTarget a rdf:Property ; rdfs:domain pa:FileRule ; rdfs:range xsd:string ; rdfs:comment \"Exact -> profile target where the transition permits it.\" .\npa:owner a rdf:Property ; rdfs:domain pa:FileRule ; rdfs:range xsd:boolean ; rdfs:comment \"Emit the owner qualifier.\" .\npa:audit a rdf:Property ; rdfs:domain pa:Rule ; rdfs:range xsd:boolean ; rdfs:comment \"Emit the audit qualifier.\" .\npa:deny a rdf:Property ; rdfs:domain pa:Rule ; rdfs:range xsd:boolean ; rdfs:comment \"Emit a deny rule; otherwise an allow rule.\" .\npa:capability a rdf:Property ; rdfs:domain pa:CapabilityRule ; rdfs:range xsd:string ; rdfs:comment \"Exact Linux capability name without CAP_.\" .\npa:domain a rdf:Property ; rdfs:domain pa:NetworkRule ; rdfs:range xsd:string ; rdfs:comment \"AppArmor address family such as inet or inet6.\" .\npa:socketType a rdf:Property ; rdfs:domain pa:Rule ; rdfs:range xsd:string ; rdfs:comment \"Socket type such as stream; exclusive with network protocol.\" .\npa:protocol a rdf:Property ; rdfs:domain pa:NetworkRule ; rdfs:range xsd:string ; rdfs:comment \"Protocol such as tcp, exclusive with socketType in AppArmor 4.1.\" .\npa:access a rdf:Property ; rdfs:domain pa:Rule ; rdfs:range xsd:string ; rdfs:comment \"Typed rule access set, comma-separated using grammar-approved tokens.\" .\npa:localIp a rdf:Property ; rdfs:domain pa:NetworkRule ; rdfs:range xsd:string ; rdfs:comment \"One exact local address, without CIDR syntax.\" .\npa:localPort a rdf:Property ; rdfs:domain pa:NetworkRule ; rdfs:range xsd:string ; rdfs:comment \"Port number or inclusive N-M range, within 0 through 65535.\" .\npa:peerIp a rdf:Property ; rdfs:domain pa:NetworkRule ; rdfs:range xsd:string ; rdfs:comment \"One exact peer address, without CIDR syntax.\" .\npa:peerPort a rdf:Property ; rdfs:domain pa:NetworkRule ; rdfs:range xsd:string ; rdfs:comment \"Peer port or inclusive range.\" .\npa:address a rdf:Property ; rdfs:domain pa:UnixRule ; rdfs:range xsd:string ; rdfs:comment \"Unix addr expression in the supported AppArmor glob language.\" .\npa:peerAddress a rdf:Property ; rdfs:domain pa:UnixRule ; rdfs:range xsd:string ; rdfs:comment \"Unix peer addr expression.\" .\npa:peerLabel a rdf:Property ; rdfs:domain pa:Rule ; rdfs:range xsd:string ; rdfs:comment \"Unix, signal or ptrace peer label expression.\" .\npa:signalSet a rdf:Property ; rdfs:domain pa:SignalRule ; rdfs:range xsd:string ; rdfs:comment \"Comma-separated AppArmor signal names.\" .\npa:mountOperation a rdf:Property ; rdfs:domain pa:MountRule ; rdfs:range xsd:string ; rdfs:comment \"mount, remount, umount or pivot_root; deny is mandatory.\" .\npa:outputProjectionJq a rdf:Property ; rdfs:domain pa:Context ; rdfs:range xsd:string ; rdfs:comment \"Only the D6 field-mask language, not arbitrary jq.\" .\npa:maxOutputBytes a rdf:Property ; rdfs:domain pa:Context ; rdfs:range xsd:positiveInteger ; rdfs:comment \"Maximum complete projected output size in exact UTF-8 bytes, including JSON framing.\" .\npa:historyRetentionTurns a rdf:Property ; rdfs:domain pa:Context ; rdfs:range xsd:nonNegativeInteger ; rdfs:comment \"Number of completed turns retained; zero retains none.\" .\npa:intentTags a rdf:Property ; rdfs:domain pa:Context ; rdfs:range xsd:string ; rdfs:comment \"Repeated BM25F intent field values.\" .\npa:loadingStrategy a rdf:Property ; rdfs:domain pa:Context ; rdfs:range xsd:string ; rdfs:comment \"always or on_demand; retrieval never authorizes a call.\" .\npa:tool a rdf:Property ; rdfs:domain pa:Call ; rdfs:range pa:Tool ; rdfs:comment \"Canonical resolved tool identity.\" .\npa:session a rdf:Property ; rdfs:domain pa:Call ; rdfs:range pa:Session ; rdfs:comment \"Trusted immutable session identity.\" .\npa:userId a rdf:Property ; rdfs:domain pa:Session ; rdfs:range xsd:string ; rdfs:comment \"Authenticated user identifier, never read from model output.\" .\npa:tenant a rdf:Property ; rdfs:domain pa:Node ; rdfs:range pa:Node ; rdfs:comment \"Trusted session or customer tenant identity.\" .\npa:active a rdf:Property ; rdfs:domain pa:Node ; rdfs:range xsd:boolean ; rdfs:comment \"Trusted active-tenant status.\" .\npa:customerId a rdf:Property ; rdfs:domain pa:Call ; rdfs:range xsd:string ; rdfs:comment \"Validated customer_id argument projected as a call fact.\" .\npa:candidateAllowed a rdf:Property ; rdfs:domain pa:Call ; rdfs:range pa:Tool ; rdfs:comment \"Untrusted tool-rule intermediate proposal, never sufficient to authorize.\" .\npa:candidateDenied a rdf:Property ; rdfs:domain pa:Call ; rdfs:range pa:Tool ; rdfs:comment \"Untrusted tool-rule intermediate refusal proposal, consumed only by trusted gate rules.\" .\npa:permits a rdf:Property ; rdfs:domain pa:Call ; rdfs:range pa:Tool ; rdfs:comment \"Derived authorization for this exact call and tool.\" .\npa:denies a rdf:Property ; rdfs:domain pa:Call ; rdfs:range pa:Tool ; rdfs:comment \"Derived refusal overriding permits.\" .\npa:conflict a rdf:Property ; rdfs:domain pa:Node ; rdfs:range xsd:string ; rdfs:comment \"Derived static or dynamic contradiction with stable diagnostic code.\" .\n\n@prefix pa: <https://example.org/toolgraph#> .\n@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .\n@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n@prefix sh: <http://www.w3.org/ns/shacl#> .\n@prefix fno: <https://w3id.org/function/ontology#> .\npa:ToolShape a sh:NodeShape ; sh:targetClass pa:Tool ;\n sh:property [ sh:path pa:name ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ; sh:pattern \"^[A-Za-z0-9_.:-]+$\" ] ;\n sh:property [ sh:path pa:subtool ; sh:class pa:Tool ] ;\n sh:property [ sh:path pa:description ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:inputShape ; sh:minCount 1 ; sh:maxCount 1 ; sh:class sh:NodeShape ] ;\n sh:property [ sh:path pa:outputShape ; sh:minCount 1 ; sh:maxCount 1 ; sh:class sh:NodeShape ] ;\n sh:property [ sh:path pa:executionProfile ; sh:minCount 1 ; sh:maxCount 1 ; sh:node pa:ExecutionShape ] ;\n sh:property [ sh:path pa:contextManagement ; sh:minCount 1 ; sh:maxCount 1 ; sh:node pa:ContextShape ] ;\n sh:property [ sh:path pa:parameterConstraints ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:title ; sh:maxCount 1 ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:readOnlyHint ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:boolean ] ;\n sh:property [ sh:path pa:destructiveHint ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:boolean ] ;\n sh:property [ sh:path pa:idempotentHint ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:boolean ] ;\n sh:property [ sh:path pa:openWorldHint ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:boolean ] ;\n sh:property [ sh:path fno:expects ; sh:minCount 1 ; sh:maxCount 1 ; sh:nodeKind sh:BlankNodeOrIRI ] ;\n sh:property [ sh:path fno:returns ; sh:minCount 1 ; sh:maxCount 1 ; sh:nodeKind sh:BlankNodeOrIRI ] .\npa:ExecutionShape a sh:NodeShape ; sh:targetClass pa:ExecutionProfile ;\n sh:property [ sh:path pa:confinementMode ; sh:minCount 1 ; sh:maxCount 1 ; sh:in ( \"aa-exec\" \"change_hat\" ) ] ;\n sh:property [ sh:path pa:profileName ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:hatName ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:egressAllowed ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:boolean ] ;\n sh:property [ sh:path pa:addressSpaceBytes ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:positiveInteger ] ;\n sh:property [ sh:path pa:fileSizeBytes ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:positiveInteger ] ;\n sh:property [ sh:path pa:processCount ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:positiveInteger ] ;\n sh:property [ sh:path pa:cpuSeconds ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:positiveInteger ] ;\n sh:property [ sh:path pa:openFileCount ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:positiveInteger ] ;\n sh:property [ sh:path pa:rule ; sh:nodeKind sh:BlankNodeOrIRI ] ;\n sh:property [ sh:path pa:subprofile ; sh:node pa:ExecutionShape ] ;\n sh:property [ sh:path pa:waiver ; sh:node pa:WaiverShape ] .\npa:WaiverShape a sh:NodeShape ; sh:targetClass pa:Waiver ;\n sh:property [ sh:path pa:waivedRule ; sh:minCount 1 ; sh:maxCount 1 ; sh:class pa:FileRule ] ;\n sh:property [ sh:path pa:reviewer ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ; sh:minLength 1 ] ;\n sh:property [ sh:path pa:rationale ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ; sh:minLength 1 ] ;\n sh:property [ sh:path pa:waiverSource ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:anyURI ] .\npa:ContextShape a sh:NodeShape ; sh:targetClass pa:Context ;\n sh:property [ sh:path pa:outputProjectionJq ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:maxOutputBytes ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:positiveInteger ] ;\n sh:property [ sh:path pa:historyRetentionTurns ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:nonNegativeInteger ] ;\n sh:property [ sh:path pa:intentTags ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:loadingStrategy ; sh:minCount 1 ; sh:maxCount 1 ; sh:in ( \"always\" \"on_demand\" ) ] .\npa:HintShape a sh:NodeShape ; sh:targetClass pa:Hint ;\n sh:property [ sh:path pa:maxEditDistance ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:nonNegativeInteger ] ;\n sh:property [ sh:path pa:alias ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:typeCoercion ; sh:minCount 1 ; sh:in ( \"none\" \"string-to-integer\" \"integer-to-string\" ) ] ;\n sh:property [ sh:path pa:caseFold ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:boolean ] ;\n sh:property [ sh:path pa:unicodeFold ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:boolean ] ;\n sh:property [ sh:path pa:taint ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:sessionBinding ; sh:maxCount 1 ; sh:nodeKind sh:IRI ] .\npa:NetworkShape a sh:NodeShape ; sh:targetClass pa:NetworkRule ;\n sh:property [ sh:path pa:domain ; sh:minCount 1 ; sh:maxCount 1 ; sh:in ( \"inet\" \"inet6\" ) ] ;\n sh:xone (\n  [ sh:property [ sh:path pa:socketType ; sh:minCount 1 ; sh:maxCount 1 ] ; sh:property [ sh:path pa:protocol ; sh:maxCount 0 ] ]\n  [ sh:property [ sh:path pa:protocol ; sh:minCount 1 ; sh:maxCount 1 ] ; sh:property [ sh:path pa:socketType ; sh:maxCount 0 ] ]\n ) ;\n sh:property [ sh:path pa:access ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:peerIp ; sh:maxCount 1 ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:peerPort ; sh:maxCount 1 ; sh:datatype xsd:string ] .\npa:FileShape a sh:NodeShape ; sh:targetClass pa:FileRule ;\n sh:property [ sh:path pa:pathGlob ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:filePermissions ; sh:maxCount 1 ; sh:datatype xsd:string ; sh:pattern \"^[rwaklm]*$\" ] ;\n sh:property [ sh:path pa:execTransition ; sh:maxCount 1 ; sh:in ( \"ix\" \"px\" \"cx\" \"ux\" \"Ux\" \"Pix\" \"Cix\" ) ] ;\n sh:property [ sh:path pa:transitionTarget ; sh:maxCount 1 ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:owner ; sh:maxCount 1 ; sh:datatype xsd:boolean ] .\npa:RuleShape a sh:NodeShape ; sh:targetClass pa:Rule ;\n sh:property [ sh:path pa:audit ; sh:maxCount 1 ; sh:datatype xsd:boolean ] ;\n sh:property [ sh:path pa:deny ; sh:maxCount 1 ; sh:datatype xsd:boolean ] .\npa:CapabilityShape a sh:NodeShape ; sh:targetClass pa:CapabilityRule ;\n sh:property [ sh:path pa:capability ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] .\npa:UnixShape a sh:NodeShape ; sh:targetClass pa:UnixRule ;\n sh:property [ sh:path pa:access ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:socketType ; sh:maxCount 1 ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:address ; sh:maxCount 1 ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:peerAddress ; sh:maxCount 1 ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:peerLabel ; sh:maxCount 1 ; sh:datatype xsd:string ] .\npa:SignalShape a sh:NodeShape ; sh:targetClass pa:SignalRule ;\n sh:property [ sh:path pa:access ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:signalSet ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:peerLabel ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] .\npa:PtraceShape a sh:NodeShape ; sh:targetClass pa:PtraceRule ;\n sh:property [ sh:path pa:access ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] ;\n sh:property [ sh:path pa:peerLabel ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:string ] .\npa:MountShape a sh:NodeShape ; sh:targetClass pa:MountRule ;\n sh:property [ sh:path pa:mountOperation ; sh:minCount 1 ; sh:maxCount 1 ; sh:in ( \"mount\" \"remount\" \"umount\" \"pivot_root\" ) ] ;\n sh:property [ sh:path pa:deny ; sh:hasValue true ; sh:minCount 1 ; sh:maxCount 1 ] .\n\n@prefix pa: <https://example.org/toolgraph#> .\n@prefix ex: <https://example.org/tools/> .\n@prefix sh: <http://www.w3.org/ns/shacl#> .\n@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n@prefix fno: <https://w3id.org/function/ontology#> .\n@prefix fnom: <https://w3id.org/function/vocabulary/mapping#> .\n@prefix owl: <http://www.w3.org/2002/07/owl#> .\n@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n@prefix dcterms: <http://purl.org/dc/terms/> .\n\nex:QueryArguments a rdfs:Class .\nex:EditArguments a rdfs:Class .\nex:QueryResult a rdfs:Class .\nex:EditResult a rdfs:Class .\nex:catalog a owl:Ontology ; dcterms:isPartOf <https://example.org/toolgraph#> ;\n rdfs:label \"Worked tool catalog\" ; rdfs:comment \"Reviewed tool profile catalog for this example.\" .\n\nex:query_customer_orders a pa:Tool, fno:Function ; rdfs:label \"query_customer_orders\" ;\n pa:name \"query_customer_orders\" ; pa:description \"Query customer orders.\" ;\n pa:title \"Customer orders\" ; pa:readOnlyHint true ; pa:destructiveHint false ;\n pa:idempotentHint true ; pa:openWorldHint false ;\n fno:expects ( ex:customerParam ex:limitParam ) ; fno:returns ( ex:ordersOutput ) ;\n pa:inputShape ex:QueryInput ; pa:outputShape ex:QueryOutput ;\n pa:executionProfile ex:DatabaseProfile ; pa:contextManagement ex:QueryContext ;\n pa:parameterConstraints \"\"\"PREFIX pa: <https://example.org/toolgraph#>\nPREFIX ex: <https://example.org/tools/>\nRULE ex:queryPermit { ?call pa:permits ex:query_customer_orders }\nWHERE {\n ?call pa:tool ex:query_customer_orders . ?call pa:session ?session .\n ?call pa:customerId ?user . ?session pa:userId ?user .\n ?session pa:tenant ?tenant . ?tenant pa:active true .\n}\nRULE ex:queryDeny { ?call pa:denies ex:query_customer_orders }\nWHERE {\n ?call pa:tool ex:query_customer_orders .\n NOT { ?call pa:permits ex:query_customer_orders }\n}\"\"\" .\nex:edit_file a pa:Tool, fno:Function ; rdfs:label \"edit_file\" ;\n pa:name \"edit_file\" ; pa:description \"Edit a workspace file.\" ;\n pa:title \"Edit file\" ; pa:readOnlyHint false ; pa:destructiveHint true ;\n pa:idempotentHint false ; pa:openWorldHint false ;\n fno:expects ( ex:pathParam ex:textParam ) ; fno:returns ( ex:editOutput ) ;\n pa:inputShape ex:EditInput ; pa:outputShape ex:EditOutput ;\n pa:executionProfile ex:FileProfile ; pa:contextManagement ex:EditContext ;\n pa:parameterConstraints \"\"\"PREFIX pa: <https://example.org/toolgraph#>\nPREFIX ex: <https://example.org/tools/>\nRULE ex:editPermit { ?call pa:permits ex:edit_file }\nWHERE {\n ?call pa:tool ex:edit_file . ?call pa:session ?session .\n ?session pa:tenant ?tenant . ?tenant pa:active true .\n}\nRULE ex:editDeny { ?call pa:denies ex:edit_file }\nWHERE { ?call pa:tool ex:edit_file . NOT { ?call pa:permits ex:edit_file } }\"\"\" .\nex:customerParam a fno:Parameter ; fno:predicate ex:customer_id ; fno:type xsd:string ; fno:required true .\nex:pathParam a fno:Parameter ; fno:predicate ex:path ; fno:type xsd:string ; fno:required true .\nex:textParam a fno:Parameter ; fno:predicate ex:text ; fno:type xsd:string ; fno:required true .\nex:limitParam a fno:Parameter ; fno:predicate ex:limit ; fno:type xsd:integer ; fno:required true .\nex:ordersOutput a fno:Output ; fno:predicate ex:orders ; fno:type xsd:string .\nex:editOutput a fno:Output ; fno:predicate ex:version ; fno:type xsd:string .\n_:queryMapping a fno:Mapping ; rdfs:label \"query implementation binding\" ; fno:function ex:query_customer_orders ; fno:implementation ex:queryImplementation ; fno:parameterMapping _:customerMap, _:limitMap ; fno:returnMapping _:ordersMap .\nex:queryImplementation a fno:Implementation ; dcterms:format \"application/vnd.purrassist.host-tool\" ; rdfs:seeAlso <urn:purrassist:host:query_customer_orders> .\n_:editMapping a fno:Mapping ; rdfs:label \"edit implementation binding\" ; fno:function ex:edit_file ; fno:implementation ex:editImplementation ; fno:parameterMapping _:pathMap, _:textMap ; fno:returnMapping _:versionMap .\nex:editImplementation a fno:Implementation ; dcterms:format \"application/vnd.purrassist.host-tool\" ; rdfs:seeAlso <urn:purrassist:host:edit_file> .\n_:customerMap a fnom:PropertyParameterMapping ; rdfs:label \"customer binding\" ; fnom:functionParameter ex:customerParam ; fnom:implementationProperty \"customer_id\" .\n_:limitMap a fnom:PropertyParameterMapping ; rdfs:label \"limit binding\" ; fnom:functionParameter ex:limitParam ; fnom:implementationProperty \"limit\" .\n_:pathMap a fnom:PropertyParameterMapping ; rdfs:label \"path binding\" ; fnom:functionParameter ex:pathParam ; fnom:implementationProperty \"path\" .\n_:textMap a fnom:PropertyParameterMapping ; rdfs:label \"text binding\" ; fnom:functionParameter ex:textParam ; fnom:implementationProperty \"text\" .\n_:ordersMap a fnom:DefaultReturnMapping ; rdfs:label \"orders binding\" ; fnom:functionOutput ex:ordersOutput ; fnom:implementationProperty \"orders\" .\n_:versionMap a fnom:DefaultReturnMapping ; rdfs:label \"version binding\" ; fnom:functionOutput ex:editOutput ; fnom:implementationProperty \"version\" .\nex:QueryInput a sh:NodeShape ; sh:targetClass ex:QueryArguments ; sh:closed true ;\n sh:property ex:customerIdShape, ex:limitShape .\nex:customerIdShape a sh:PropertyShape ; sh:path ex:customer_id ; sh:datatype xsd:string ; sh:pattern \"^customer-[0-9]+$\" ; sh:minCount 1 ; sh:maxCount 1 .\n<< ex:customerIdShape sh:path ex:customer_id ~ ex:customerClaim >> .\nex:customerClaim a pa:Hint ; pa:maxEditDistance 0 ; pa:alias \"customer\" ;\n pa:typeCoercion \"none\" ; pa:caseFold false ; pa:unicodeFold false ;\n pa:taint \"personal-data\" ; pa:sessionBinding pa:userId .\nex:limitShape a sh:PropertyShape ; sh:path ex:limit ; sh:datatype xsd:integer ; sh:minInclusive 1 ; sh:maxInclusive 100 ; sh:minCount 1 ; sh:maxCount 1 .\n<< ex:limitShape sh:path ex:limit ~ ex:limitClaim >> .\nex:limitClaim a pa:Hint ; pa:maxEditDistance 0 ; pa:typeCoercion \"string-to-integer\" ; pa:caseFold false ; pa:unicodeFold false .\nex:EditInput a sh:NodeShape ; sh:targetClass ex:EditArguments ; sh:closed true ; sh:property ex:pathShape, ex:textShape .\nex:pathShape a sh:PropertyShape ; sh:path ex:path ; sh:datatype xsd:string ; sh:minCount 1 ; sh:maxCount 1 .\nex:textShape a sh:PropertyShape ; sh:path ex:text ; sh:datatype xsd:string ; sh:minCount 1 ; sh:maxCount 1 .\n<< ex:pathShape sh:path ex:path ~ ex:pathClaim >> .\nex:pathClaim a pa:Hint ; pa:maxEditDistance 0 ; pa:typeCoercion \"none\" ;\n pa:caseFold false ; pa:unicodeFold false ; pa:taint \"untrusted-path\" .\nex:QueryOutput a sh:NodeShape ; sh:targetClass ex:QueryResult ; sh:closed true ;\n sh:property [ sh:path ex:orders ; sh:datatype xsd:string ; sh:minCount 1 ; sh:maxCount 1 ] .\nex:EditOutput a sh:NodeShape ; sh:targetClass ex:EditResult ; sh:closed true ;\n sh:property [ sh:path ex:version ; sh:datatype xsd:string ; sh:minCount 1 ; sh:maxCount 1 ] .\nex:QueryContext a pa:Context ; pa:outputProjectionJq \".orders\" ; pa:maxOutputBytes 4096 ;\n pa:historyRetentionTurns 2 ; pa:intentTags \"customer\", \"orders\" ; pa:loadingStrategy \"on_demand\" .\nex:EditContext a pa:Context ; pa:outputProjectionJq \".version\" ; pa:maxOutputBytes 1024 ;\n pa:historyRetentionTurns 1 ; pa:intentTags \"edit\", \"file\" ; pa:loadingStrategy \"always\" .\nex:DatabaseProfile a pa:ExecutionProfile ; pa:confinementMode \"change_hat\" ;\n pa:profileName \"purrassist.tool.query_customer_orders\" ; pa:hatName \"query_customer_orders\" ;\n pa:egressAllowed true ; pa:addressSpaceBytes 268435456 ; pa:fileSizeBytes 1048576 ;\n pa:processCount 16 ; pa:cpuSeconds 5 ; pa:openFileCount 64 ;\n pa:rule ex:DatabaseNetwork, ex:TemporaryRead, ex:SshDeny, ex:EnvDeny,\n ex:SignalRule, ex:PtraceRule, ex:UnixRule, ex:MountDeny, ex:RemountDeny, ex:UmountDeny, ex:PivotDeny .\nex:FileProfile a pa:ExecutionProfile ; pa:confinementMode \"aa-exec\" ;\n pa:profileName \"purrassist.tool.edit_file\" ; pa:hatName \"edit_file\" ;\n pa:egressAllowed false ; pa:addressSpaceBytes 268435456 ; pa:fileSizeBytes 1048576 ;\n pa:processCount 16 ; pa:cpuSeconds 5 ; pa:openFileCount 64 ;\n pa:rule ex:WorkspaceFile, ex:TemporaryFile, ex:SshDeny, ex:EnvDeny,\n ex:SignalRule, ex:PtraceRule, ex:UnixRule, ex:MountDeny, ex:RemountDeny, ex:UmountDeny, ex:PivotDeny .\nex:WorkspaceFile a pa:FileRule ; pa:pathGlob \"@{WORKSPACE_ROOT}/**\" ; pa:filePermissions \"rwkl\" ; pa:owner true .\nex:TemporaryRead a pa:FileRule ; pa:pathGlob \"@{TOOL_TMP}/**\" ; pa:filePermissions \"r\" ; pa:owner true .\nex:TemporaryFile a pa:FileRule ; pa:pathGlob \"@{TOOL_TMP}/**\" ; pa:filePermissions \"rwk\" ; pa:owner true .\nex:SshDeny a pa:FileRule ; pa:pathGlob \"@{HOME}/.ssh/**\" ; pa:filePermissions \"rw\" ; pa:execTransition \"ix\" ; pa:deny true ; pa:audit true .\nex:EnvDeny a pa:FileRule ; pa:pathGlob \"/**/.env*\" ; pa:filePermissions \"rw\" ; pa:execTransition \"ix\" ; pa:deny true ; pa:audit true .\nex:DatabaseNetwork a pa:NetworkRule ; pa:domain \"inet\" ; pa:socketType \"stream\" ; pa:access \"connect\" ; pa:peerIp \"10.0.0.7\" ; pa:peerPort \"5432\" .\nex:UnixRule a pa:UnixRule ; pa:access \"send,receive\" ; pa:socketType \"stream\" ; pa:address \"@@{TOOL_TMP}/ipc\" ; pa:peerLabel \"purrassist.tools\" ; pa:peerAddress \"@@{TOOL_TMP}/host\" .\nex:SignalRule a pa:SignalRule ; pa:access \"receive\" ; pa:signalSet \"term,int\" ; pa:peerLabel \"purrassist.tools\" .\nex:PtraceRule a pa:PtraceRule ; pa:access \"trace,read,readby,tracedby\" ; pa:peerLabel \"**\" ; pa:deny true .\nex:MountDeny a pa:MountRule ; pa:mountOperation \"mount\" ; pa:deny true .\nex:RemountDeny a pa:MountRule ; pa:mountOperation \"remount\" ; pa:deny true .\nex:UmountDeny a pa:MountRule ; pa:mountOperation \"umount\" ; pa:deny true .\nex:PivotDeny a pa:MountRule ; pa:mountOperation \"pivot_root\" ; pa:deny true .",
      "lossProfile": "purrassist-toolgraph-v1"
    }
  }
}
```

### GBNF and signatures — specified

This worked grammar is the canonical generated subset for `hermes.v1`. It
admits one call, uses the existing envelope's canonical print branch, and
allows arbitrary valid JSON string values for the two tools. All other model
families use their existing `CompiledGrammar` call production as the envelope;
GBNF lowering must preserve terminals and slot bindings from that production,
not maintain a second hand-written envelope registry. Recoverable malformed
forms are excluded from constrained decoding, which emits canonical calls.
The compiler refuses dialect productions or schema constructs it cannot lower
soundly and completely modulo the explicitly registered loss profile.

```gbnf
root ::= query | edit
query ::= "<tool_call>\n{\"name\":\"query_customer_orders\",\"arguments\":{" ws "\"customer_id\":" ws customer ws "," ws "\"limit\":" ws limit ws "}}\n</tool_call>"
edit ::= "<tool_call>\n{\"name\":\"edit_file\",\"arguments\":{" ws "\"path\":" ws string ws "," ws "\"text\":" ws string ws "}}\n</tool_call>"
customer ::= "\"customer-" [0-9]+ "\""
limit ::= [1-9] | [1-9] [0-9] | "100"
string ::= "\"" character* "\""
character ::= [^"\\\x00-\x1f] | "\\" (["\\/bfnrt] | "u" hex hex hex hex)
hex ::= [0-9a-fA-F]
ws ::= [ \t\n\r]*
```

The object-member order above is the compiler's canonical order and is not a
claim that JSON itself requires ordering. The compiler must admit all
schema-valid values in its declared canonical serialization, including escaped
characters, without dropping arguments. Labeled post-checks enforce constraints
beyond context-free syntax. UTF-16 surrogate pairing and the core's exact
Unicode string validity remain mandatory schema/JSON post-checks.

The compact lane's versioned, UTF-8 `CompactSignature` for the example is exactly
the following two LF-separated lines, with one final LF. Signatures are a new
specified projection, not an existing `DeclarationForm`. Optional parameters
use `?`, unions use `|`, and arrays use `[]`; all identifiers are quoted when
they do not match the core name grammar. Unsupported recursive types are a
typed refusal, rather than truncated descriptions.

```text
query_customer_orders(customer_id:string,limit:integer)->{orders:string}
edit_file(path:string,text:string)->{version:string}
```

### Projection loss profile and mandatory checks — specified

Every lane returns `purrdf_core::loss::LossLedger`, a registered caller profile,
and an ordered set of mandatory checks. Runtime entries use the native fields
`code`, `from`, `to`, `note`, and `location`; location identifies the exact
source tool, shape, rule or reifier. Duplicate codes at different source
locations are separate runtime entries. Contract entries have unique codes.
No ad-hoc JSON replacement for `LossLedger` is introduced. The public profile
id for this version is `purrassist-toolgraph-v1` and binds its code table and
source/target representation pairs. `check_ledger_sound` must run under the
registered profile from upstream #748.

The following table is an entry-by-entry specification. A row whose source set
contains several nodes emits one runtime entry per named source node. `Q` means
`query_customer_orders`; `E` means `edit_file`. `M` means the complete MCP
Extended Manifest including its RDF bundle; `A` AppArmor; `G` GBNF; `C`
CompactSignature; `P` every model declaration family. All entries originate
in `rdf-1.2-toolgraph`; target ids are respectively `mcp-extended-v1`,
`apparmor-4.1`, `llama-gbnf`, `compact-signature-v1`, and the exact
`DeclarationForm`/family pair. The native ledger notes name the omitted
constraint and check id; the table is not itself serialized ledger JSON.

| Code | Target | Exact source nodes in this example | Mandatory check before/after execution |
|---|---|---|---|
| `authorization-not-expressible` | A, G, C, P | Q and E `parameterConstraints` | `authorize-call`: governed SRL over fresh immutable trusted facts; deny overrides permit |
| `session-binding-not-expressible` | A, G, C, P | `customerClaim/sessionBinding` | `bind-session`: validated customer_id equals authenticated userId, same active tenant |
| `taint-not-expressible` | A, G, C, P | `customerClaim/taint`, `pathClaim/taint` | `taint-policy`: labels enter trusted gate facts and cannot be removed by model output |
| `recovery-hints-not-expressible` | A, G, C, P | `customerClaim`, `limitClaim`, `pathClaim` | `recovery-tolerance`: aliases, distance, coercion and folding checked against recovery evidence |
| `confinement-not-expressible` | G, C, P | `DatabaseProfile`, `FileProfile` | `confine-worker`: exact admitted AppArmor label, features and root bindings before dispatch |
| `resource-ceiling-not-expressible` | G, C, P | five ceiling properties on each execution profile | `confine-worker`: exact rlimits and governed engine ceilings, no truncation or rounding |
| `output-shape-not-expressible` | A, G, P | `QueryOutput`, `EditOutput` | `validate-output`: full shape check before releasing or projecting output |
| `input-shape-not-expressible` | A | `QueryInput`, `EditInput` | `validate-input`: complete candidate against original shapes before authorization |
| `context-contract-not-expressible` | A, G, C, P | `QueryContext`, `EditContext` | `project-context`: field mask after output validation, exact byte cap and retention on release |
| `tool-annotations-not-expressible` | A, G, C, P | the title and four hints on Q and E | `verify-effects`: admitted implementation agrees; hints never confer permission |
| `rdf-binding-not-expressible` | A, G, C, P | Q/E FnO mappings, lists and source reifiers | `bind-manifest`: receipt pins graph, tool, implementation, source and projection digests |
| `description-not-expressible` | A, G, C | Q/E description | `bind-manifest`: preserve original descriptions in the immutable bundle/receipt; no authorization effect |
| `json-string-validity-not-expressible` | G | EditInput.path, EditInput.text | `validate-input`: exact JSON/Unicode validation, including surrogate pairing |
| `schema-keyword-not-expressible` | C | input/output `sh:closed`, every `sh:minCount`/`sh:maxCount`, `customerIdShape/sh:pattern` and `limitShape` bounds | `validate-input`, `validate-output`: all cardinality and closed-shape constraints |

The M lane has an empty loss ledger for this example: all source triples,
reifiers, rule strings, presence information and declaration bytes are in its
required bundle. The A lane has no confinement or rlimit loss; those constraints
are represented exactly or synthesis fails. The G lane has no loss for the
string and integer datatypes and the customer pattern and limit bounds, required argument keys or closed input objects, which
its grammar represents. The C lane preserves argument/output type names and
field names, but its shape-keyword row records cardinality/closure semantics
that TypeScript-like signatures do not enforce. Every P lane preserves its
input JSON Schema and descriptions; the common rows record all other omitted
semantics. No security rule is merely discarded: its graph reference and
mandatory enforcement remain bound into `Projection<T>` and the receipt.

When another source adds patterns, bounds, disjunctions, RDF term identity,
ordering or unsupported schema keywords, the compiler must either represent
them exactly, add a registered source-located loss plus a real enforcing check,
or refuse. An unknown code, absent enforcing check or orphaned check is a
compiler error. The same rule applies to non-security metadata: preserve the
source and report the loss explicitly. Removing a constraint from an artifact
without updating its ledger must fail the mutation acceptance test. A socket
scope cannot be repaired by an after-generation argument check and therefore
has no eligible loss code: D4 requires synthesis refusal.

## Model-family injection matrix — specified

This adapter profile fixes exact placement and bytes; it does not claim a
universal proprietary provider chat template. The host passes declaration
objects to provider `tools` fields where available, and passes rendered strings
to the named message location for local chat templates. It never guesses a
family from an incomplete name. Every listed family requires an explicit
caller selection and the frozen dialect id. Qwen 2.5 and non-Coder Qwen 3 use
Hermes; Qwen3-Coder uses its XML declaration and call envelope. A Llama 4
configuration is admitted only when its selected chat template uses the same
`llama3-json.v1` envelope; a different template is `UnsupportedFamily`.

Define `Oq` and `Oe` as the following two complete declaration objects, each
without a final LF. These are the exact bytes from the existing renderer with
the input-schema key order shown here. The RDF → JSON Schema lane must freeze
this canonical ordering for the worked fixture; alternate raw declaration
ordering is preserved on decode but cannot alter canonical artifacts.

```json
{"type":"function","function":{"name":"query_customer_orders","description":"Query customer orders.","parameters":{"type":"object","properties":{"customer_id":{"type":"string","pattern":"^customer-[0-9]+$"},"limit":{"type":"integer","minimum":1,"maximum":100}},"required":["customer_id","limit"],"additionalProperties":false}}}
```

```json
{"type":"function","function":{"name":"edit_file","description":"Edit a workspace file.","parameters":{"type":"object","properties":{"path":{"type":"string"},"text":{"type":"string"}},"required":["path","text"],"additionalProperties":false}}}
```

`Aq` and `Ae` are the exact Anthropic renderer outputs, without final LF:

```json
{"name":"query_customer_orders","description":"Query customer orders.","input_schema":{"type":"object","properties":{"customer_id":{"type":"string","pattern":"^customer-[0-9]+$"},"limit":{"type":"integer","minimum":1,"maximum":100}},"required":["customer_id","limit"],"additionalProperties":false}}
```

```json
{"name":"edit_file","description":"Edit a workspace file.","input_schema":{"type":"object","properties":{"path":{"type":"string"},"text":{"type":"string"}},"required":["path","text"],"additionalProperties":false}}
```

`Xq` and `Xe` are the exact Qwen3-Coder renderer outputs, without the fence's
final LF:

```text
<function>
<name>query_customer_orders</name>
<description>Query customer orders.</description>
<parameters>{"type":"object","properties":{"customer_id":{"type":"string","pattern":"^customer-[0-9]+$"},"limit":{"type":"integer","minimum":1,"maximum":100}},"required":["customer_id","limit"],"additionalProperties":false}</parameters>
</function>
```

```text
<function>
<name>edit_file</name>
<description>Edit a workspace file.</description>
<parameters>{"type":"object","properties":{"path":{"type":"string"},"text":{"type":"string"}},"required":["path","text"],"additionalProperties":false}</parameters>
</function>
```

`Dq` and `De` are the exact DeepSeek-V3.1 renderer outputs, without final LF:

```text
### query_customer_orders
Description: Query customer orders.

Parameters: {"type":"object","properties":{"customer_id":{"type":"string","pattern":"^customer-[0-9]+$"},"limit":{"type":"integer","minimum":1,"maximum":100}},"required":["customer_id","limit"],"additionalProperties":false}
```

```text
### edit_file
Description: Edit a workspace file.

Parameters: {"type":"object","properties":{"path":{"type":"string"},"text":{"type":"string"}},"required":["path","text"],"additionalProperties":false}
```

In the table, `+` concatenates bytes, `LF` is the single byte 0x0a, and quoted
literals contain exactly their displayed characters, with no implicit spaces.
Array entries are ordered by canonical tool name: E precedes Q. `J` means
`"[" + Oe + "," + Oq + "]"`; this formula expands to exact bytes, not an
elliptical payload. Each renderer is invoked rather than reimplemented by the
projector. `F(x)` means the bytes `0x60 0x60 0x60`, then `json`, LF, `x`, LF,
and those same three fence bytes; DeepSeek-V3's renderer supplies that wrapper.

| Family | Existing form and exact injection bytes/placement | Recovery dialect | Loss entries |
|---|---|---|---|
| Claude Opus / Sonnet | Anthropic Messages request `tools` field: `"[" + Ae + "," + Aq + "]"`; no duplicated system text | `anthropic-tool-use.v1` | P rows above |
| Qwen 2.5 / Qwen 3 (Hermes template) | `DeclarationForm::Hermes`, system content `"<tools>" + LF + Oe + LF + Oq + LF + "</tools>"` | `hermes.v1` | P rows |
| Qwen3-Coder | `DeclarationForm::Qwen3CoderXml`, system content `"<tools>" + LF + Xe + LF + Xq + LF + "</tools>"` | `qwen3-coder-xml.v1` | P rows |
| Hermes | `DeclarationForm::Hermes`, same system bytes as Qwen Hermes row | `hermes.v1` | P rows |
| Llama 3.x / admitted Llama 4 | `DeclarationForm::Llama3Json`, system content `Oe + LF + Oq` | `llama3-json.v1` | P rows |
| Mistral v3 | `DeclarationForm::MistralV3`, tool-list segment `"[AVAILABLE_TOOLS]" + J + "[/AVAILABLE_TOOLS]"` in the caller's template system/tool-instruction position | `mistral-v3.v1` | P rows |
| Mistral v11 | `DeclarationForm::MistralV11`, same available-tools segment, selected v11 chat template | `mistral-v11.v1` | P rows |
| DeepSeek V3 | `DeclarationForm::DeepSeekV3`, system content `F(Oe) + LF + F(Oq)` | `deepseek-v3.v1` | P rows |
| DeepSeek V3.1 | `DeclarationForm::DeepSeekV31`, system content `"## Tools" + LF + De + LF + LF + Dq` | `deepseek-v3.1.v1` | P rows |
| CompactTypeScript | New compact/TypeScript preamble in system content, then `DeclarationForm::OpenAi` request `tools` field `J`; the preamble is the two exact compact lines above, each with LF | `openai-tool-calls.v1` | C rows for preamble; P rows for declaration |

CompactTypeScript reuses `purrdf_shapes::emit_typescript` for the richer
TypeScript lane; its emitted type declarations are attached to the preamble
when that lane is selected, with their own returned losses. There is no
existing `DeclarationForm::CompactTypeScript`. The compact signature alone
must not be mislabeled as PurRDF's complete TypeScript output. The OpenAI tools
binding above remains mandatory so the exact schema and recovery envelope are
shared; a type preamble cannot establish runtime validity.

The combined CompactTypeScript projection uses one target for its tool-graph entries:
`prompt.compact-typescript/openai-tool-calls.v1`. Its profile registers the
union of C and P meanings. Each occurrence identifies the original source
constraint and the affected preamble or declaration component. Original upstream
ledgers remain unchanged; projectors validate each against its own registered
source/target pair.

The prompt projector will report exact injected UTF-8 bytes and a separate
conservative token upper bound, D5. For every row,
`bytes = UTF8(exact injected content).len()` plus all selected declaration,
wrapper and separator bytes; a provider tools array counts the serialized
request field value. A caller's explicit prompt byte ceiling will refuse a
larger representation, regardless of any token measurement. The projector will
never truncate a declaration or drop an always-loaded tool.

The fallback `tokens <= bytes` will apply only to an explicitly admitted
byte-level tokenizer whose encoding assigns at most one token per input byte,
without implicit special tokens or preprocessing that expands the counted
representation. All caller-added framing and special tokens must be included
in the admitted accounting. Without those assumptions or a verified applicable
tokenizer, a token-bound claim will fail explicitly; an exact byte measurement
will remain available. A caller-supplied deterministic tokenizer may tighten
the prompt token bound and will record its identity and counted representation.
Exact token counts, conservative token upper bounds and exact bytes will have
distinct receipt fields. No float estimates, hard-coded model token ratios or
unverified tokenizer claims will enter admission. An omitted on-demand declaration emits `tool-selection-omitted` from
`rdf-1.2-toolgraph` to that exact family representation, with source tool
location and an enforced `bind-selected-tools` check: recovery and dispatch
use only the selected immutable tool set. This selection loss grants no
authority to call an omitted tool. The prompt byte ceiling includes any TypeScript preamble actually emitted.
Tool-result limits apply separately through the exact byte contract above.

### Deterministic retrieval — specified

`always` tools are included first, in tool-name order, and consume the exact
byte budget. `on_demand` candidates are ranked with `purrdf_text::PreparedCorpus`
and `RankingProfile`, profile `purrdf-bm25f-fixed-v2`. One document is one
canonical tool IRI; language is an explicit caller input. NFC/UAX #29 analysis,
fixed-point arithmetic and all corpus statistics are those of the selected
PurRDF profile, recorded in the manifest. Fields are `intentTags` weight 4 and
`description` weight 1, both with length-normalization `b=0`, and `k1=1.2` as
an exact fixed-point decimal. Queries use the same analyzer, unique terms in
lexical order, and no ambient locale. Scores sort descending; ties sort by
canonical tool IRI bytes ascending, then tool name bytes. Retrieval adds whole
declarations in that order until the next declaration would exceed the cap;
it never skips an earlier higher-ranked candidate to fill spare bytes. A
caller can request a smaller positive top-k ceiling, which is recorded.

For query `orders`, the example corpus has two documents. E's tags are
`edit file`, description `Edit a workspace file.`; Q's tags are `customer
orders`, description `Query customer orders.`. E is always loaded. Q has one
`orders` occurrence in each field, weighted frequency 5 with `b=0`, document
frequency 1 in a corpus of 2. Under the profile's positive IDF and saturation,
Q has a positive score and E scores zero. Q is the first on-demand tool and
is added if its complete declaration fits after E; otherwise the projection
returns E with a source-located `tool-selection-omitted` loss for Q,
recording its rank, required bytes and remaining bytes. If the caller explicitly
requires Q, that call makes Q mandatory and the same shortage is
`BudgetExhausted`. Mandatory E alone exceeding the cap is always an error.
With two equal-scoring on-demand tools, the lower canonical IRI bytes win.
The receipt records query terms, profile/corpus fingerprints, ranking scores,
tie-breaks and chosen tool ids. Ranking does not bypass policy admission,
validate missing arguments or infer session authority.

## Verification record — specified acceptance, not execution claims

Issue #32 requires extraction of every Turtle, SRL, JSON, GBNF and AppArmor
block. Turtle is parsed with `purrdf_rdf::parse_dataset` in Turtle 1.2 mode;
SRL is checked through `srl::check` at stratified level; JSON is strictly
parsed; GBNF passes llama.cpp's parser or a named equivalent; AppArmor passes
`apparmor_parser -QK --kernel-features /etc/apparmor.d/abi/4.0-ip`
with `--warn=all --Werror` on parser 4.1.7 without loading kernel policy.
Record the feature file's SHA-256/provenance and the exact IP/port and abstract
address DFA evidence; a default-feature negative neighbor must not be accepted
as equivalent evidence. Both specification documents pass `prose-check`. The
PR records actual command results and parser versions. Those checks validate this specification's
examples, not the unimplemented loader or confinement runtime. Phase 3 adds
the semantic and behavioral gates in TOOLS_PLAN.md; Phase 4 closes all
cross-lane, host and native/WASM acceptance.


## Validated single-result projection — implemented

`ContextManagement::project_result` accepts a caller-prepared output `Schema`,
an immutable original JSON `Node` and its governor. It validates the complete
original before masking, applies the existing four-form `FieldMask`, serializes
the complete projected stream through core's shared JSON cursor/escaping writer,
and calls the canonical `OutputByteLimit` once before publishing any artifact.

The explicit representation is compact JSON Lines: every emitted value has one
trailing LF, while zero values produce zero bytes. There is no outer array.
D6 fixes the four mask forms but did not fix a stream delimiter; this callable
path fixes LF framing explicitly. Quotes, escapes, UTF-8 and every LF count.
A path or field object emits one value. Missing/null named lookups propagate
null; other non-object traversal fails. Field objects retain mask key order.
`.[]` emits root-array members in order. `.name[]?` emits a selected array's
members, or zero values for a missing/null/non-array selection. Selected duplicate
names fail as ambiguous rather than choosing a first/last value or emitting null.
Whole selected values preserve nested duplicate occurrences and numeric spelling.

Success and refusal retain the original node, original mask, full validation
report when available, last attempted stage and explicit transformations for
null insertion, optional omission and completed mask application. These are
transformations rather than inferred source facts. Source DOM/schema/context
preparation is caller-owned. Native resource/cancellation refusal is distinct
from the exact-byte semantic ceiling. Oversize output is refused intact; no
partial or truncated stream is returned. Token evidence cannot override it.
New projection/trace/output buffers use admitted scratch owners; a refused
unpublished serialized stream releases its storage.

Core's narrow `JsonField`/`JsonLine` borrowed views and
`json::lines_to_json_scratch` reuse the same cursor and escaping authority as
ordinary node emission. They copy no input DOM and preserve existing node
serialization bytes and resource scheduling. The `project_result` example reads
a bounded stdin envelope containing `format`, `context`, `outputSchema` and
`result`, delegates version/legacy/mode validation to the existing context codec,
and writes only complete checked bytes to stdout. Refusals go to stderr. This
input envelope is not an admitted graph manifest.

History pruning, full RDF/SHACL/SRL/GTS bundle admission/roundtrip, graph-backed
registry/dispatch, authorization/confinement, graph/proof-bound receipts and all
remaining #33/#36/#37/#47/#39 acceptance remain required. This operation provides
no graph authority and adds no host/facade dependency on the unpublished crate.
