# Independent review: issue 211 tests, head cd41a70

Reviewer: independent read-only review (delivery-reviewer, Opus), diff 335b12f..cd41a70 (138 insertions, 0 deletions, one test file). Judged from source; no build run by the reviewer.

VERDICT: PASS at head cd41a70.

- NamedGraph: the populated TriG GRAPH statement is refused at load.rs (statement path), with subject EditResult and predicate rdf:type; valid neighbour is the worked example loaded as TriG.
- Reifier: a reifier with the four hint properties and no pa:Hint type reaches Construct::Reifier at load.rs:903; valid neighbour is the worked example.
- RuntimeFact: pa:userId as a predicate and a pa:Session class each reach the runtime-fact refusal; neighbour is the worked example.
- FunctionWithoutTool: a bare fno:Function without a tool role is refused at the function loop; neighbour is the worked example.
- ComplexPath: sh:path [] reaches the non-IRI path refusal; neighbour is the worked example. Optional: a sequence path would be a more faithful complex path.
- The implementer's report is accurate: reifier lines 207 and 210 are unreached from Turtle or TriG, because the reifier contract guarantees a triple term with an IRI predicate.
- No existing test changed; the helper unsupported_construct is test-only and adds no fallback.

Optional, not blocking: the sequence-path form for ComplexPath; the class form of RuntimeFact could also assert subject and predicate; the named-graph reifier refusal at load.rs:205 has no test (not asked by issue 211).
