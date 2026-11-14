/// A small xorshift generator with a fixed seed: the only source of variation.
struct Rng(u64);

impl Rng {
    const fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value in `0..bound`.
    fn below(&mut self, bound: u32) -> u32 {
        let high = u32::try_from(self.next() >> 32).expect("upper half fits in 32 bits");
        high % bound
    }

    fn flip(&mut self) -> bool {
        self.below(2) == 1
    }
}

/// One generated graph: its Turtle source and what it exercises.
struct Generated {
    turtle: String,
    /// The number of `sh:equals` constraints injected, each a ledgered loss.
    expected_losses: u32,
    /// Labels of the vocabulary variations the graph exercises.
    seen: BTreeSet<&'static str>,
}

/// Every variation label the fixed set must exercise.
const VARIATIONS: [&str; 8] = [
    "optional parameter",
    "required parameter",
    "hinted parameter",
    "unhinted parameter",
    "integer parameter",
    "integer output",
    "read-only tool",
    "mutating tool",
];

/// The Turtle under construction, with the generator state that shapes it.
struct Builder {
    rng: Rng,
    out: String,
    seen: BTreeSet<&'static str>,
    losses: u32,
}

impl Builder {
    fn line(&mut self, text: &str) {
        self.out.push_str(text);
        self.out.push('\n');
    }

    /// A tool's header: annotations, FnO lists, shape and context links, and its policy.
    fn header(&mut self, t: u32, read_only: bool, egress: bool, params: u32) {
        let titled = self.rng.flip();
        let expects: Vec<String> = (0..params).map(|i| format!("ex:t{t}Param{i}")).collect();
        let mut head = format!(
            "ex:tool_{t} a pa:Tool, fno:Function ; rdfs:label \"tool_{t}\" ; pa:name \"tool_{t}\" ; \
             pa:description \"Generated tool {t}.\" ; "
        );
        if titled {
            write!(head, "pa:title \"Tool {t}\" ; ").expect("string write");
        }
        write!(
            head,
            "pa:readOnlyHint {read_only} ; pa:destructiveHint {} ; pa:idempotentHint {read_only} ; \
             pa:openWorldHint {egress} ; fno:expects ( {} ) ; fno:returns ( ex:t{t}Output0 ) ; \
             pa:inputShape ex:t{t}Input ; pa:outputShape ex:t{t}Output ; \
             pa:executionProfile ex:t{t}Profile ; pa:contextManagement ex:t{t}Context ; \
             pa:parameterConstraints \"\"\"PREFIX pa: <https://example.org/toolgraph#>\n\
PREFIX ex: <https://example.org/tools/>\n\
RULE ex:t{t}Permit {{ ?call pa:permits ex:tool_{t} }}\n\
WHERE {{\n ?call pa:tool ex:tool_{t} . ?call pa:session ?session .\n \
?session pa:tenant ?tenant . ?tenant pa:active true .\n}}\n\
RULE ex:t{t}Deny {{ ?call pa:denies ex:tool_{t} }}\n\
WHERE {{ ?call pa:tool ex:tool_{t} . NOT {{ ?call pa:permits ex:tool_{t} }} }}\"\"\" .",
            !read_only,
            expects.join(" "),
        )
        .expect("string write");
        self.line(&head);
    }

    /// One parameter: its FnO declaration, SHACL shape, optional hint and mapping.
    fn parameter(&mut self, t: u32, i: u32, params: u32) {
        let required = self.rng.below(3) != 0;
        let integer = self.rng.flip();
        let hinted = self.rng.flip();
        self.seen.insert(if required {
            "required parameter"
        } else {
            "optional parameter"
        });
        self.seen.insert(if hinted {
            "hinted parameter"
        } else {
            "unhinted parameter"
        });
        if integer {
            self.seen.insert("integer parameter");
        }
        let datatype = if integer { "xsd:integer" } else { "xsd:string" };
        let min_count = u8::from(required);
        let mut shape = format!(
            "ex:t{t}Shape{i} a sh:PropertyShape ; sh:path ex:t{t}p{i} ; sh:datatype {datatype} ; \
             sh:minCount {min_count} ; sh:maxCount 1"
        );
        if integer && self.rng.flip() {
            let upper = 10 + self.rng.below(90);
            write!(shape, " ; sh:minInclusive 1 ; sh:maxInclusive {upper}").expect("string write");
        }
        if params > 1 && self.rng.below(4) == 0 {
            let other = (i + 1) % params;
            write!(shape, " ; sh:equals ex:t{t}p{other}").expect("string write");
            self.losses += 1;
        }
        shape.push_str(" .");
        self.line(&format!(
            "ex:t{t}Param{i} a fno:Parameter ; fno:predicate ex:t{t}p{i} ; fno:type {datatype} ; \
             fno:required {required} ."
        ));
        self.line(&shape);
        if hinted {
            self.hint(t, i, integer);
        }
        self.line(&format!(
            "_:t{t}Map{i} a fnom:PropertyParameterMapping ; rdfs:label \"t{t} binding {i}\" ; \
             fnom:functionParameter ex:t{t}Param{i} ; fnom:implementationProperty \"p{i}\" ."
        ));
    }

    /// The reified hint on parameter `i`, with optional alias and taint.
    fn hint(&mut self, t: u32, i: u32, integer: bool) {
        let coercion = if integer { "string-to-integer" } else { "none" };
        let distance = self.rng.below(3);
        let case_fold = self.rng.flip();
        let unicode_fold = self.rng.flip();
        let mut claim = format!(
            "ex:t{t}Claim{i} a pa:Hint ; pa:maxEditDistance {distance} ; \
             pa:typeCoercion \"{coercion}\" ; pa:caseFold {case_fold} ; pa:unicodeFold {unicode_fold}"
        );
        if self.rng.flip() {
            write!(claim, " ; pa:alias \"alias{t}x{i}\"").expect("string write");
        }
        if self.rng.flip() {
            write!(claim, " ; pa:taint \"taint{t}x{i}\"").expect("string write");
        }
        claim.push_str(" .");
        self.line(&format!(
            "<< ex:t{t}Shape{i} sh:path ex:t{t}p{i} ~ ex:t{t}Claim{i} >> ."
        ));
        self.line(&claim);
    }

    /// The single output (the loader requires exactly one), its shapes and mapping.
    fn output(&mut self, t: u32, params: u32) {
        let integer = self.rng.flip();
        if integer {
            self.seen.insert("integer output");
        }
        let kind = if integer { "xsd:integer" } else { "xsd:string" };
        let maps: Vec<String> = (0..params).map(|i| format!("_:t{t}Map{i}")).collect();
        let inputs: Vec<String> = (0..params).map(|i| format!("ex:t{t}Shape{i}")).collect();
        self.line(&format!(
            "ex:t{t}Output0 a fno:Output ; fno:predicate ex:t{t}out0 ; fno:type {kind} ."
        ));
        self.line(&format!(
            "_:t{t}Return0 a fnom:DefaultReturnMapping ; rdfs:label \"t{t} output 0\" ; \
             fnom:functionOutput ex:t{t}Output0 ; fnom:implementationProperty \"out0\" ."
        ));
        self.line(&format!(
            "_:t{t}Mapping a fno:Mapping ; rdfs:label \"t{t} implementation binding\" ; \
             fno:function ex:tool_{t} ; fno:implementation ex:t{t}Implementation ; \
             fno:parameterMapping {} ; fno:returnMapping _:t{t}Return0 .",
            maps.join(", ")
        ));
        self.line(&format!(
            "ex:t{t}Implementation a fno:Implementation ; \
             dcterms:format \"application/vnd.purrassist.host-tool\" ; \
             rdfs:seeAlso <urn:purrassist:host:tool_{t}> ."
        ));
        self.line(&format!(
            "ex:T{t}Arguments a rdfs:Class .\nex:T{t}Result a rdfs:Class ."
        ));
        self.line(&format!(
            "ex:t{t}Input a sh:NodeShape ; sh:targetClass ex:T{t}Arguments ; sh:closed true ; \
             sh:property {} .",
            inputs.join(", ")
        ));
        self.line(&format!(
            "ex:t{t}Output a sh:NodeShape ; sh:targetClass ex:T{t}Result ; sh:closed true ; \
             sh:property [ sh:path ex:t{t}out0 ; sh:datatype {kind} ; sh:minCount 1 ; sh:maxCount 1 ] ."
        ));
    }

    /// The context block (the projection is required) and the execution profile.
    fn context_and_profile(&mut self, t: u32, read_only: bool, egress: bool) {
        let strategy = if self.rng.flip() { "always" } else { "on_demand" };
        let max_bytes = 256 << self.rng.below(5);
        let turns = 1 + self.rng.below(4);
        self.line(&format!(
            "ex:t{t}Context a pa:Context ; pa:outputProjectionJq \".t{t}out0\" ; \
             pa:maxOutputBytes {max_bytes} ; pa:historyRetentionTurns {turns} ; \
             pa:intentTags \"tag{t}a\", \"tag{t}b\" ; pa:loadingStrategy \"{strategy}\" ."
        ));
        let mode = if self.rng.flip() {
            "change_hat"
        } else {
            "aa-exec"
        };
        let kind_rules = match (read_only, egress) {
            (true, true) => "ex:TemporaryRead, ex:DatabaseNetwork",
            (true, false) => "ex:TemporaryRead",
            (false, _) => "ex:WorkspaceFile, ex:TemporaryFile",
        };
        self.line(&format!(
            "ex:t{t}Profile a pa:ExecutionProfile ; pa:confinementMode \"{mode}\" ; \
             pa:profileName \"purrassist.tool.tool_{t}\" ; pa:hatName \"tool_{t}\" ; \
             pa:egressAllowed {egress} ; pa:addressSpaceBytes 268435456 ; pa:fileSizeBytes 1048576 ; \
             pa:processCount 16 ; pa:cpuSeconds 5 ; pa:openFileCount 64 ; \
             pa:rule {kind_rules}, {COMMON_RULE_LIST} ."
        ));
    }

    /// One whole tool; returns whether it is read-only and whether it has egress.
    fn tool(&mut self, t: u32) -> (bool, bool) {
        let read_only = self.rng.flip();
        let egress = read_only && self.rng.flip();
        let params = 1 + self.rng.below(3);
        self.seen.insert(if read_only {
            "read-only tool"
        } else {
            "mutating tool"
        });
        self.header(t, read_only, egress, params);
        for i in 0..params {
            self.parameter(t, i, params);
        }
        self.output(t, params);
        self.context_and_profile(t, read_only, egress);
        (read_only, egress)
    }
}

/// The `index`th generated graph: a Turtle document the loader accepts.
fn generate(index: u32) -> Generated {
    let spread = u64::from(index) + 1;
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ ((spread << 32) | spread));
    // Discard the weakly mixed first outputs of a structured seed.
    for _ in 0..4 {
        rng.next();
    }
    let tools = 1 + rng.below(3);
    let mut builder = Builder {
        rng,
        out: String::new(),
        seen: BTreeSet::new(),
        losses: 0,
    };
    let (mut writes, mut reads, mut network) = (false, false, false);
    for t in 0..tools {
        let (read_only, egress) = builder.tool(t);
        writes |= !read_only;
        reads |= read_only;
        network |= egress;
    }
    let mut turtle = String::from(PROLOGUE);
    turtle.push_str(CATALOG);
    turtle.push_str(COMMON_RULES);
