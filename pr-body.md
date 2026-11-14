Test each UnsupportedConstruct sub-kind of the tool graph loader (issue 211, follow-up to PR 195).

Five tests, one per sub-kind (NamedGraph, Reifier, RuntimeFact, FunctionWithoutTool, ComplexPath), each with an invalid input and the unedited worked example as the valid neighbour. Two Reifier refusal lines (207 and 210) are not reachable from Turtle or TriG, because the reifier contract guarantees a triple term with an IRI predicate; they are not forced.

Review: independent review PASS at cd41a70, .stage/unsupported-subkinds/reviews/cd41a70-review.md. make check exit 0 at cd41a70 (local).

Refs #191, closes #211
