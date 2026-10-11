# R2 review (G1, nested unknown-member policy)

1. write.rs: only the ObjectSpec path uses nested_unknown_call (declared value, else DropWithLoss); the ToolDefinition path still calls unknown_call(tool.unknown), so top-level mutating/read_only with no key emit no call and keep the effects default. Correct for Error/Drop/Pass/None.
2. Tests match the plan: case 1 (Resolved turn, call Unresolved UnknownParameter /inner/zz; stronger than "not Resolved"), 2, 3, 4 (== case 2: arguments and losses equal), Outer2 and read_only (BestGuess, losses /top /inner/zz /items/0/zz, rule drop-unknown, original text, from_node values). Churn: only dims() and one nested helper; plan-listed facade/README/derive_support sites have no nested ObjectSpec equality (grep verified).
3. Fail-before: with HEAD write.rs in a /tmp copy, nested_without_an_unknown_key_drops_with_recorded_loss fails; other 6 pass (they declare/inherit drop). Scratch deleted.
4. Docs (lib.rs section, README, ObjectSpec note, ToolArguments/ToolParameter docs, INTEGRATION.md) accurate and consistent with behaviour.
5. cargo test -p core -p derive -p purrassist --locked all green; clippy --workspace -D warnings clean; make hygiene OK.

VERDICT: PASS
