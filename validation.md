# Validation index — issue #1, PR #24 (float_cmp under the release toolchain)

- Failure: release-qualification run 38351629747 on 464306c, job `package`, step `Verify workspace` (`make check` on 1.98.0), clippy::float_cmp at crates/core/src/tool_call/from_node.rs:336. Publish run 38351629930: the gate failed and publish was skipped. crates.io still shows 0.0.0 for all six crates.
- Reproduction: `rust-validate 1.98.0 cargo clippy --workspace --all-targets -- -D warnings` gave exactly one error.
- After fac0d98: `rust-validate 1.98.0 make check` exit 0 (110 test groups).
- review-debt.md PASS; merge-plan-review.md PASS.
