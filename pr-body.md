Adds a deterministic generated round-trip and ledger-soundness test over the vocabulary (sixth piece of #33): eight generated tool graphs with a fixed-seed PRNG, no new dependency. For each: decode(encode(g)) equals g except the ledgered losses; every ledger entry is sound under the registered profile; two encodes are byte-identical.

Known limits (independent review, optional findings, not fixed here): projections and output count are fixed by the loader; annotation presence is not varied; the span test reads the generator labels, not the loaded graph.

Review: independent review PASS at d5a297a, .stage/tool-graph-generated-round-trip/reviews/d5a297a-review.md. make check exit 0 at d5a297a (local).

Refs #33, closes #205
