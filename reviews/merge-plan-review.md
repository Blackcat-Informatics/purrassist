# Stage 3 merge-plan review: PR #24 (issue #1)

- Reviewed: squash notes `/home/paudley/Active/purrassist/.worktrees/1-complete-purrassist-0-1-0-plan-and.squash_notes`
- Head `fac0d98`, base `main` `464306c`
- Evidence read: `crates/core/src/tool_call/from_node.rs`, `.stage/complete-purrassist-0-1-0-plan-and/validation.md`, `pr-checks.txt`, `Cargo.toml` (workspace lints), `rust-toolchain.toml`, `.github/workflows/{release-qualification,verify,publish}.yml`, `AGENTS.md`

## Coverage

### 1. The test still means the same thing

`from_node.rs:336-340` now reads:

```rust
assert_eq!(
    (above_midpoint.parse::<f64>().unwrap() as f32).to_bits(),
    1.0_f32.to_bits(),
    "the double-rounding path would differ"
);
```

The assertion still claims exact equality: parsing through `f64` and then narrowing to `f32` lands exactly on `1.0`. Comparing bit patterns differs from `==` in only two cases. One is signed zeros (`+0.0 == -0.0`, but their bits differ). The other is NaN (`NaN != NaN`, but its bits can match). Neither case applies when the expected value is `1.0`, which has one bit pattern and is not NaN. So the new comparison is exactly as strict as the old one for this value. It also matches the style of the assertion above it (line 334, `f32::from_bits(1.0_f32.to_bits() + 1)`).

`#[allow(clippy::cast_possible_truncation)]` on line 324 is still needed because the `as f32` cast remains. Only the test changed; the code under test is untouched.

### 2. No other exact float comparison should trip `clippy::float_cmp`

The workspace enables `pedantic = "warn"` (`Cargo.toml:33`), which includes `float_cmp`, and release qualification passes `-D warnings`. I searched `crates/` for float equality.

- `from_node.rs:326-328,342`, `from_node.rs:174-175` (doctest), and `crates/core/tests/tool_definitions.rs:571` compare `Result<f32/f64, FromNodeError>` values, not bare floats. `float_cmp` only fires when the compared operands have a float type.
- `crates/core/src/embeddings.rs:198`, `crates/core/src/intent.rs:263` and `crates/host/src/tools/embeddings.rs:217` compare against `0.0`. `float_cmp` explicitly allows comparisons with zero.
- `crates/core/tests/intent.rs:96` uses an `abs()` tolerance.
- The remaining float-like `assert_eq!` hits compare strings, number spellings, or integer conversions.
- The workspace has no `allow(clippy::float_cmp)` anywhere, so nothing is being suppressed.

This agrees with `validation.md`: `rust-validate 1.98.0 cargo clippy --workspace --all-targets -- -D warnings` found exactly one error, and `rust-validate 1.98.0 make check` exits 0 after `fac0d98`.

### 3. The squash notes are honest and clean

- They reference the issue with `Refs #1` and no closing keyword. They say #1 stays open until publication, acceptance of the downloaded package, and the GitHub release. That is correct, because nothing has been published.
- There is no attribution trailer, no agent author or credit, and no GhpRsq trailer. The only trailer is `Defect-Class: none`.
- They say nothing about hosted-CI accounts or payments.
- Their factual claims match the repository:
  - Release qualification sets `RUSTUP_TOOLCHAIN: 1.98.0` and runs `make check` (`release-qualification.yml:22,43`).
  - Local `make check` uses `nightly-2026-09-14` (`rust-toolchain.toml:4`).
  - The Windows 1.98 lane lints only host and cli (`verify.yml:60`).
  - The publish gate requires qualification for the same commit (`release-qualification.yml:8-9`).
  - The failed run IDs and "nothing reached crates.io" match `validation.md`.
- The file stat (1 file, +2/-2) fits the one-assertion change.

## Findings

No BLOCKING findings.

ADVISORY: PR CI still does not run full-workspace Clippy on 1.98 before a release tag, so a similar stable-only lint could reach the tag again. Closing that gap would mean broadening `verify.yml:60` beyond `-p purrassist-host -p purrassist-cli`. That is outside this PR's authorized scope. The notes already name the gap, so nothing in this PR needs to change. It is worth a follow-up issue.

VERDICT: PASS
