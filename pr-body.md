PARTIAL DELIVERY of issue 36: this PR is the CompactSignature lane only. It does not close #36; the GBNF compiler, the model-family prompt projector and BM25F routing remain open parts of #36. Routing waits on issue 221.

## Spec decision: worked-example line order

TOOLS.md's compact paragraph stated the two signature lines in an order no sort produces (the query line before the edit line), and gave no ordering rule for them. The rule is now stated in words first: the lines are ordered by tool name bytes. The worked-example lines follow that rule, with `edit_file` first. Searched for any other holder of the old order: `git grep` for the two compact lines finds them only in `docs/tool-graph/TOOLS.md` in the base tree, with no fixture, golden file or test holding the old order. The only ordering rule in the document governs retrieval tie-breaks, not these lines. This is recorded as a spec decision for ratification.

## What the lane does

- `lanes::compact` derives one signature line per tool from the existing `mcp::encode` projection, in tool-name byte order.
- Every omitted keyword, every depth-limited object level and every upstream schema-compiler loss is recorded in the loss ledger and checked against the registered profile before return.
- A `$ref` or an untyped schema is refused with `CompactError::Unsupported`.
- Identifiers are quoted only outside the core name grammar (`is_valid_name`).

## Review and gate

- Independent review of the lane: findings on identifier quoting and the name-order test were fixed in 70e3519; a helper-census finding (a duplicate of `purrassist_core::json::pointer_segment`) and a dangling doc line were fixed in 5045636 and 1da7ec0; the delta review of 5045636 blocked only on that doc line, which 1da7ec0 removes.
- `make check` exit 0 at 1da7ec0 (local).
