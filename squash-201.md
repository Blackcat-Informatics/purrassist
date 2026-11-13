Merge PR 201: load tool graphs through purrdf 4.0.2 governed parse and FnO catalog entry points (issue 200, fourth piece of #33).

Budget: native work units convert at 64 per work unit, remainder carried; the default Standard budget is unchanged. Admitted Turtle sizes measured on the worked example and a bulk document: Exact 27,648 bytes; Quick 63,724; Standard 265,429 (work binds); Deep and Maximum 1,049,171 (scratch binds).

Review: whole code delta 27751a9..6f044b4 PASS (.stage/tool-graph-load-through-purrdf-s/reviews/governed-27751a9-6f044b4-code-review.md); docs delta 633668a..6f044b4 PASS (reviews/governed-6f044b4-docs-review.md); merge delta af09914..08dfd04 PASS (reviews/merge-08dfd04-review.md).
Gate: make check exit 0 at 08dfd04 (local).
Hosted checks at merge: corpus, fuzz, native ubuntu, qualification, release-gate, reuse, wasm, windows-engines all green; native macOS fails as in issue 180 (operator runner), not a gate for this change.
Stacked after PR 199 (merged 4170f2d); the merge of main brings it in.
