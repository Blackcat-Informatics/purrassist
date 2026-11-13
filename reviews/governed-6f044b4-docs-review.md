# Independent re-review (documentation outcome): issue 200, head 6f044b4

Reviewer: independent read-only review (delivery-reviewer, Opus), delta 633668a..6f044b4 (6 files).

VERDICT: PASS at head 6f044b4.

(a) Five-level table (Exact, Quick, Standard, Deep, Maximum) is identical in TOOLS.md, README, graph.rs and CHANGELOG: 27,648 / 63,724 / 265,429 / 1,049,171 / 1,049,171 bytes; binding work, work, work, scratch, scratch.
(b) Every number reproduces from .stage/tool-graph-load-through-purrdf-s/probe2.log (worked example 9,518 bytes: work 33,191, scratch 569,829; bulk 1,049,171 bytes: work 4,084,594, scratch 66,933,394).
(c) Binding resource checked against crates/core/src/contract.rs (work 100k, 250k, 1M, 10M, 50M; scratch 64 MiB and input 16 MiB at every level). Standard refuses at work 999,984; Deep and Maximum refuse on scratch at n=115.
(d) "about 205", 290 KB, 14 MB and related figures are gone from docs and comments.
(e) TOOLS.md line 22 no longer contradicts the 64-to-1 conversion text.
(f) No probe file committed; the delta is six documentation and comment files plus one added assertion.
(g) graph_load.rs adds an iris equality assertion and changes no existing assertion.
(h) prose-check exit 0 on the changed docs.

Non-blocking nits: the repetition step in TOOLS.md and graph.rs reads "about 9,100 bytes"; the probe differences are 9,019 to 9,365 bytes, so "about 9,000 to 9,400 bytes" is accurate. Some wrapping and capitalisation after colons in README, graph.rs and CHANGELOG.
