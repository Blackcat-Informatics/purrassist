Apply the newer compiler clippy rewrite in predicate_needs_test (issue 217).

Sites of the new compiler lint found by a workspace clippy with --keep-going and -D warnings (run with the PR 216 change merged in locally): exactly two in total.
- crates/core/src/tool_call/failed_prefix.rs:49, carried by PR 216 (issue 215), equivalence reviewed PASS at db4e925.
- crates/helper-census/src/source.rs:962, this PR: else if any { !children.is_empty() && all } else { false } becomes any && !children.is_empty() && all. When any is true the value is the old one; when any is false the expression short-circuits to false, never evaluating is_empty or all. is_ident is a read-only comparison. Covered by test_only_items_and_test_directories_are_not_walked (source.rs:1299). Review PASS at ef9d3b2.

No third site: the keep-going workspace clippy run exited 0 once both rewrites are in.

Refs #217
