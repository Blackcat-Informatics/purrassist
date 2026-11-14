# Independent review: issue 207 fix, head 1e304ea

Reviewer: independent read-only review (delivery-reviewer, Opus), diff 1eef154..1e304ea (load.rs: one arm; graph_load.rs: one test). Judged from source; no build run by the reviewer.

VERDICT: PASS at head 1e304ea.

- The new Statement arm reads row.s and row.p from purrdf-core 4.0.2 FnoReadError::Statement; the reported subject and predicate are the offending row, not the tool.
- The generic fallback and the Field, List and Returns arms are unchanged.
- The test asserts the subject and the predicate exactly, with the unedited worked example as the valid neighbour, and the edit is applied through edited(), which asserts the target text exists.
- Before the fix the predicate would be None and the subject the vocabulary namespace, so the equality assertions fail on the pre-fix code.
- No existing assertion changed; the diff adds 22 lines and removes none.

Optional, not blocking: the arm renders the predicate with show(), so a non-IRI predicate prints as a blank node or literal; the test covers only the Unclaimed reason.
