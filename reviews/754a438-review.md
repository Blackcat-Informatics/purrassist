# Independent review: issue 209 fix, head 754a438

Reviewer: independent read-only review (delivery-reviewer, Opus), diff 1eef154..754a438 (load.rs, problem.rs, TOOLS.md, graph_load.rs). Judged from source; no build run by the reviewer.

VERDICT: PASS at head 754a438.

- The named-graphs accessor exists in purrdf-core 4.0.2 (dataset_view.rs) and RdfDataset overrides it to include declared empty graphs; the TriG parser declares empty graphs.
- A named graph with statements still fails at the existing quad refusal first; default-graph input is unchanged.
- The refusal is UnsupportedConstruct with the graph IRI as subject and NamedGraph as the construct; no panic or unwrap is introduced.
- The test loads the unedited worked example as TriG and expects success, then appends an empty GRAPH block and asserts the refusal; before the fix the document would load, so the test fails on the pre-fix code.
- Docs in problem.rs and TOOLS.md describe the behaviour; no existing assertion changed.

Optional, not blocking: only the first declared empty graph is named in the refusal; one over-long TOOLS.md line near the new paragraph.
