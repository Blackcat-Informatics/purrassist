# R5 review (G5, A5, G6)

## Scope
Uncommitted diff vs HEAD 69c6b5c: ir.rs (`held`, `adopt_keyed`, unit tests), xml.rs (`Scope::canonical`, two adopt call sites), resolve.rs (`decoded` by span), tests in grammar_codecs / tool_call_rules / tool_call_goldens / tool_call_cost, one new golden.

## Evidence
- `cargo test -q -p purrassist-core --test tool_call_goldens --test tool_call_rules --test grammar_codecs --test tool_call_cost`: all pass. `--lib ir`: 9 pass. `cargo clippy -p purrassist-core --all-targets -- -D warnings`: clean.
- (6) `git status` under tests/data/tool-call-goldens: only the new golden is untracked; no existing golden changed.
- (3) CLI p6 (`recover-tool-calls --policy tolerant`): `decode-typed` for `/xs/0` is 43..45 and `/xs/1` is 74..78 (matches the spec). The golden and the Rust test assert both by name.
- (1) `held` bookkeeping: `held` is only used on frame trees, which are always created empty (xml.rs 924, 1059, 1646). `wrap_item` builds a fresh tree, so there is no inherited `held`. `ArgTree::from_parameters` starts from `empty_object`. No code mutates `root` members after adopt (grep: no remove/retain/insert on argument trees). Index bookkeeping holds in each adopt path:
  - `first_array` is the push position, and members are only appended.
  - The marker index into `collected` is stable because `collected` is append-only.
  - A stand-in value returns before `held` changes, as before.
  - Scalars count 1, arrays count len, and a merge adds len.
  - `Scope::canonical` differs from the written name only for `Declared` scopes, and `collect` is only set for undeclared scopes, so alias merging into one member in collect mode cannot arise.
- (2) Leaf-pointer consumers: `.leaves` is read only at recognizer.rs 1450 (`marker_cut`), recognizer.rs 2443 (digest encoding), resolve.rs 2510 (`decoded`, now by span) and 3131 (`unmodeled_sites`, by span). Evidence destinations come from the resolver's pointers, not from leaves. The digest now hashes alias spelling with shared indices, which is deterministic and harmless.
- (4) Mutation evidence is plausible by reading. Reverting `decoded` to pointer matching returns the first `/xs` leaf for both items, so the golden's 74..78 fails. Passing the written name instead of the canonical one to `adopt_keyed` gives `/ex/0`, so the alias tests fail. I did not execute the mutations (read-only review).
- (5) Cost test is deterministic: it compares work and charged-scratch counts, not wall time. Measured: work/byte ratio is flat (3659362/62052 vs 29244362/496052, within 1.05x). Scratch grows about 36x for 8x (quadratic, as documented). The `adopt` linearity is checked by a member-inspection counter at k=32768 (`adopt_is_constant_in_the_members_held`).

## Findings
REQUIRED: none.

ADVISORY
1. Runtime. `repeated_parameters_cost_is_linear` took 48 s in a debug run (32 s in the batch), well above any other test in tool_call_cost. The time comes from the pre-existing quadratic scratch copy (about 14e9 bytes charged at k=8000). `make test` is `cargo test --workspace` in debug, and there is no slower lane for this file (only vendored corpora are `test = false`). R10 should remove the cost; until then consider k up to 4000 (about 4x cheaper), or accept it.
2. R6 coupling. `marker_cut` (recognizer.rs ~1450) resolves `root.pointer(&leaf.pointer)` for Json-origin leaves. With shared numbering, an alias leaf `/ex/1` now points past the end of the `ex` member (which holds 1 item), so `pointer` returns None and a reasoning marker inside that JSON string is treated as data. Previously `/ex/0` resolved. The same defect already exists for same-name duplicates (G7), so this is not a new class, but R6 must key by leaf span (or the alias case) rather than pointer, and include an alias-spelled repetition in its test.
3. Test gap. The unit tests do not exercise `adopt_keyed` with an alias and `collect = true`. It is unreachable today (see Evidence 1), but the doc comment on `adopt_keyed` implies it works. Optional: say so in the comment.
4. The `MEMBERS_INSPECTED` counter is only incremented in the merge branch, so the test bounds the merge path; the non-merge path is O(1) by inspection (a hash lookup and a push).

## R9 compatibility items (public API)
- `ArgTree` has a new private field `held`, so external struct-literal construction of `ArgTree { .. }` no longer compiles. All other fields remain pub. No in-repo literal exists outside `ArgTree::value`.
- `ArgTree::value` and `ArgTree::empty_object` are `pub` and lose `const` (they build a `HashMap`). Anything calling them in const context breaks (none in-repo).
- `ArgTree` equality ignores `held` (custom `PartialEq` on `Held`). A tree whose `root` was mutated externally keeps stale `held`, but `adopt*` is `pub(crate)`, so external mutation cannot feed it.
- Behavior change: leaf pointers of alias repetitions now use shared indices (`/xs/0`, `/ex/1`, `/xs/2`), and `decode-typed` evidence for repeated scalar items under a near-miss tool name cites each item's own span. Both should be in the changelog or PR text and the p6 claim.

VERDICT: PASS
