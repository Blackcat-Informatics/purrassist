Refuse a declared named graph that holds no statements (follow-up to PR 195, found by its fresh review).

A TriG input with an empty GRAPH block was loaded and the empty graph silently dropped. The loader now refuses the first declared named graph with a typed UnsupportedConstruct (NamedGraph), naming the graph. Default-graph input is unchanged; named graphs with statements still hit the existing refusal first.

Review: independent review PASS at 754a438, .stage/empty-named-graph/reviews/754a438-review.md. make check exit 0 at 754a438 (local).

Refs #191, closes #209
