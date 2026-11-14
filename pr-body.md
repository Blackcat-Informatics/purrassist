Use the worked example as the Parse test valid neighbour (issue 210, follow-up to PR 195).

The neighbour <urn:a> <urn:b> <urn:c> . parses but is refused, so the test proved parse validity only. It is now load(worked()), which the loader accepts. The LoadError::Parse assertion for the invalid input is unchanged. Merge of main brings the two newer-compiler lint rewrites (PRs 216 and 218); the test file is unaffected.

Review: independent review PASS at 925a3cc, .stage/parse-neighbour/reviews/925a3cc-review.md, written before the merge. My own reading of the diff agrees: one added line, six removed, in graph_load.rs. make check exit 0 at 925a3cc (local).

Refs #191, closes #210
