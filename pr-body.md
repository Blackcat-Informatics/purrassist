Name the subject and predicate of an FnO-view statement refusal (fix forward from PR 195, merged as d808673).

Fresh review of the merged PR 195 change found that the strict FnO reader refuses an unclaimed statement with a generic subject and no predicate, so the loader did not name them as issue 191 requires. This adds the Statement arm to fno_error and a test that asserts the subject and predicate exactly, with the unedited worked example as the valid neighbour.

Review: independent review PASS at 1e304ea, .stage/fno-statement-subject/reviews/1e304ea-review.md. make check exit 0 at 1e304ea (local).

Refs #191, closes #207
