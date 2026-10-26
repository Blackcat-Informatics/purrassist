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
pa:maxOutputTokens a rdf:Property ; rdfs:domain pa:Context ; rdfs:range xsd:positiveInteger ; rdfs:comment "Maximum output token count, bounded conservatively by bytes." .
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
 sh:property [ sh:path pa:maxOutputTokens ; sh:minCount 1 ; sh:maxCount 1 ; sh:datatype xsd:positiveInteger ] ;
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
ex:QueryContext a pa:Context ; pa:outputProjectionJq ".orders" ; pa:maxOutputTokens 4096 ;
 pa:historyRetentionTurns 2 ; pa:intentTags "customer", "orders" ; pa:loadingStrategy "on_demand" .
ex:EditContext a pa:Context ; pa:outputProjectionJq ".version" ; pa:maxOutputTokens 1024 ;
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
ex:UnixRule a pa:UnixRule ; pa:access "send,receive" ; pa:socketType "stream" ; pa:address "@{TOOL_TMP}/ipc" ; pa:peerLabel "purrassist.tools" ; pa:peerAddress "@{TOOL_TMP}/host" .
ex:SignalRule a pa:SignalRule ; pa:access "receive" ; pa:signalSet "term,int" ; pa:peerLabel "purrassist.tools" .
ex:PtraceRule a pa:PtraceRule ; pa:access "trace,read,readby,tracedby" ; pa:peerLabel "**" ; pa:deny true .
ex:MountDeny a pa:MountRule ; pa:mountOperation "mount" ; pa:deny true .
ex:RemountDeny a pa:MountRule ; pa:mountOperation "remount" ; pa:deny true .
ex:UmountDeny a pa:MountRule ; pa:mountOperation "umount" ; pa:deny true .
ex:PivotDeny a pa:MountRule ; pa:mountOperation "pivot_root" ; pa:deny true .
```
