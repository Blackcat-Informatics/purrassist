# R2 review (G2, G16, A6-doc) - uncommitted tree vs HEAD c1fa81d

## Scope
Plan "### R2", gap-analysis G2 + Appendix, security R1/A6, completion-audit F1, remediation-plan-review F1-F3, tasks/R2-derivation.md.
Read: scope.rs (new), xml.rs / edit.rs / ir.rs / shape.rs / contract.rs diffs, goldens, tests, docs.

## Evidence
- Built CLI (`target/debug/purrassist recover-tool-calls`), Appendix inputs against a tools file with the Appendix tools plus rm/save:
  - p9r default: exit 0, resolved, `{"children":[{"name":"a","children":[{"name":"b"}]}]}`; `--effort exact`: exit 0, resolved.
  - p10_ref_plan and p10_inline_plan: exit 0, both `{"name":"party","outfits":[{"name":"Alice","size":3},{"name":"Bob","size":4}]}` (size integer).
  - rm/keep (`keep: array of $ref{path}`): exit 0, `{"path":"/tmp/x","keep":[{"path":"/home/user/important"}]}`, verdict run.
  - outer-array collision (save): exit 0, `{"tags":["work"],"notes":[{"text":"buy milk","tags":["home"]}]}`.
  - opaque rm (free-form item): exit 4, best_guess, verdict nothing_usable, `{"keep":[{}],"path":"/home/user/important"}`, rules nested-parameter, unmodeled-scope-boundary, array-item, framing-newline x2, stray-closer. `--policy tolerant`: still exit 4, nothing_usable.
- Goldens: all 7 named goldens plus repeated_member_collects captured arguments/outcome/ledger rules equal the plan and R2-derivation tables (checked by reading each JSON). Golden dir: exactly the 1 deletion (`..._are_siblings`) and 8 additions, no modified golden.
- Adversarial: mutually recursive `$defs` (a<->b via members and arrays), 3-step `$ref` chain, anyOf [string, object, null] member, anyOf string|integer member given nested markup (opaque, unresolved schema_validation, no panic): all explicit and correct. Pure `$ref` cycle: invalid_input (pre-existing check). 3000-long ref chain, 2000 chained defs, 2000x50 anyOf, 5000-deep anyOf: `BudgetExceeded(Work|Depth)` in <0.1 s, no panic/hang. 400-def chains: explicit Depth/Work.
- New operator: code review shows it is chosen only when the popped frame's scope is `Scope::Opaque` (and priced only for Opaque in `cheapest`); Declared/Schema frames keep MissingParameterClose.
- Corpus: jsonl diff is add-only (10 lines, 0 removed); head tally 12629 correct / 0 incorrect / 5547 abstained = base 12621/0/5545 + 8 correct + 2 abstained (the 2 truth-Unresolved free-object cases); abstention recall 5415/5415. So base cases are unchanged (A4 holds by arithmetic); no per-case base replay was run.
- Runs: `cargo test -q -p purrassist-core --test tool_call_goldens --test tool_call_rules --test tool_adapters --test grammar_codecs --test tool_call_cost --test tool_call_recover --test correction_evaluation` all ok; doctests ok (374); `cargo clippy -q --workspace --all-targets -- -D warnings` clean.
- API docs: Nesting, ScopeMember, ScopeNode, ScopeGraph have Rustdoc + doctests; SPDX header present on scope.rs; `ArgumentShape.scopes`, `ParameterShape.nesting`, `EditOperator::UnmodeledScopeBoundary` documented; RULES.md row (cost 4) and qwen3 / INTEGRATION docs updated; "$ref -> sibling" limit removed. `Budget::depth` Rustdoc carries the stack-guard sentence (A6-doc).

## Real callers/consumers
`ScopeGraph` built in adapter.rs per tool and reached by xml.rs Reader (`graph`, `Frame.scope`, `nesting`, `parameter`, `cheapest`); operator flows through OPERATOR_TABLE into the rule registry (tool_call_rules passes); CLI/MCP reach it via ToolSet::from_openai.

## Findings
REQUIRED: none.

ADVISORY
1. Deviation (a), collect: a repeated array member of a `$ref`/anyOf scope merges silently (`adopt_collecting`) and records no `tool_call.parameter.collect`, whereas the inline twin (`tree`) records it for the same input (probe: inline ledger ends with parameter.collect, `rtree` does not). Result values are identical and collect is a 0-cost non-guess, so no Resolved-from-guess risk, but the ledger is less complete than the modeled path. Scalar members are not affected (collect requires every non-null branch typed `array`; a union with a non-array branch stays non-collecting). Suggest recording the rule or noting the asymmetry in the golden description.
2. Deviation (b): `Scope::of_object` makes any typed object with zero parameters + PassThrough Opaque, so closing it at an outer opener changed from the non-guess MissingParameterClose to the guess UnmodeledScopeBoundary for existing free-form object parameters/items. No existing golden changed and corpus incorrect is 0; this is documented in the codec docs, and is the more honest reading, but it is a behaviour change to modeled shapes; list it in the R9 compatibility notes.
3. Deviation (c): accepted. The zero-repair reading (path inside the item) leaves the required top-level `path` missing, so it is not a rival valid reading for `rm`; the golden is best_guess, nothing_usable, exit 4, never resolved. Cost is registry cost 4 (+6 stray-closer = 10), matching R2-derivation; the plan's "cost 2" is the base cost.
4. Deviation (d) / G16: the plan asked that `d` levels resolve and `d+1` exhaust. Delivered: codec `d-1` items fit and `d` exhaust; whole call fits `(d-1)/3` (~42 item levels at depth 128) because Depth is charged again on read-back. Both boundaries are asserted (`nested_item_depth_is_a_stack_guard`), exhaustion is explicit with no partial value. The `(d-1)/3` figure lives only in the test comment; add one sentence to INTEGRATION.md or the Budget::depth Rustdoc.
5. A4 evidence is by tally arithmetic only and is not recorded in validation.md; record the base/head tallies there.

VERDICT: PASS

## Recheck

- Advisory 1 (collect evidence): discharged. Probe with two outer items, the first holding two inner `children` and the second three, run against the inline tool and the `$ref` twin through the CLI. Both give identical arguments and three identical `tool_call.parameter.collect` records. Text matches ("collected N occurrence(s) of parameter `children` as array items"). Destinations match (`.../arguments/children`, `/children/0/children`, `/children/1/children`), so markers re-root under nesting and item shifts (`shift_item`, prefix re-rooting in `adopt_collecting`). The key span is the first occurrence's name in both; offsets differ only by the one-byte longer tool name. The shared `record_collect` is used by both paths.
- Advisory 4 (depth): discharged. Sentences are in `Budget::depth` Rustdoc and INTEGRATION.md (codec `depth - 1`, whole call about `(depth - 1) / 3`, explicit `BudgetExceeded(Depth)`), naming the test.
- Goldens dir: unchanged apart from the original deletion plus the new files (repeated_member_collects is new in this tree). No other golden modified.
- `cargo test -q -p purrassist-core --test tool_call_goldens --test tool_call_rules`: 109 and 42 passed.
- New public items (`ArgTree.collected`, `CollectedMember`) have Rustdoc with a doctest. Add them to the R9 compatibility list.

VERDICT: PASS
