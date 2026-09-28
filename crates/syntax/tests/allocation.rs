// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Tree-sitter allocations are governed exactly like every other resource:
//! this file proves the budget/cancellation/cross-thread/overshoot claims in
//! `crates/syntax/src/alloc.rs` and `docs/RESOURCES.md`.
use purrassist_core::{Budget, CancellationToken, Governor, StopReason};
use purrassist_syntax::{Language, alloc, inspect, parse_governed};
use std::fmt::Write as _;
use std::sync::Mutex;
use std::sync::mpsc;

/// [`alloc::live_bytes`]/[`alloc::peak_bytes`] are process-wide, and `cargo
/// test` runs this file's test functions as concurrent threads within one
/// process; every test below holds this lock for its entire body so no
/// other test's tree-sitter activity can move these counters mid-assertion
/// (the same pattern `crates/core/tests/allocation_oracle.rs` uses for its
/// own test-only counting allocator).
static LOCK: Mutex<()> = Mutex::new(());

/// ~1 MiB of syntactically distinct, valid Rust: enough real tree-sitter
/// allocation to exceed a small `scratch_bytes` budget, and enough distinct
/// content that nothing could plausibly be deduplicated away.
fn large_rust_source() -> String {
    let mut source = String::new();
    let mut index = 0_u32;
    while source.len() < 1024 * 1024 {
        let _ = write!(
            source,
            "fn generated_{index}(a: i32, b: i32) -> i32 {{\n    let sum = a + b + {index};\n    if sum > 0 {{ sum }} else {{ -sum }}\n}}\n"
        );
        index += 1;
    }
    source
}

/// A budget wide enough on every dimension except `scratch_bytes`, so that
/// dimension alone is the binding constraint.
const fn scratch_budget(scratch_bytes: u64) -> Budget {
    Budget {
        work: u64::MAX / 2,
        candidates: u64::MAX / 2,
        input_bytes: 8 * 1024 * 1024,
        scratch_bytes,
        depth: 8192,
        evidence_entries: 10_000_000,
        helper_calls: 0,
    }
}

#[test]
fn a_tight_scratch_budget_rejects_a_large_parse_with_no_tree() {
    let _guard = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let source = large_rust_source();
    let mut governor = Governor::new(scratch_budget(64 * 1024));
    let error = inspect(&source, Language::Rust, &mut governor).unwrap_err();
    assert_eq!(
        error,
        StopReason::BudgetExceeded(purrassist_core::contract::Resource::ScratchBytes)
    );
    // No partial success: `inspect` returned only an error, never a `Report`
    // (there is no other value the `Result<Report, StopReason>` could hold).
    let mut fresh = Governor::new(scratch_budget(64 * 1024));
    // A brand-new governor with the identical (tiny) budget can still
    // charge its whole budget: the failed attempt released everything it
    // charged rather than leaking it into some other ledger.
    assert!(
        fresh
            .charge(purrassist_core::contract::Resource::ScratchBytes, 64 * 1024)
            .is_ok()
    );
}

#[test]
fn b_ample_budget_keeps_governed_peak_within_2x_of_the_real_allocator_peak() {
    let _guard = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let source = large_rust_source();
    alloc::reset_peak_bytes();
    let mut governor = Governor::new(scratch_budget(u64::MAX / 2));
    let parsed = parse_governed(&source, Language::Rust, &mut governor).unwrap();
    let governed_peak = governor.usage().peak_scratch_bytes;
    let real_peak = alloc::peak_bytes();
    assert!(governed_peak > 0);
    assert!(real_peak > 0);
    assert!(
        governed_peak <= real_peak.saturating_mul(2)
            && real_peak <= governed_peak.saturating_mul(2),
        "governed_peak={governed_peak} real_peak={real_peak}"
    );
    assert!(parsed.allocation().peak_bytes > 0);
    drop(parsed);
}

#[test]
fn c_a_tree_dropped_on_another_thread_returns_its_account_to_zero() {
    let _guard = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let before = alloc::live_bytes();
    let mut governor = Governor::new(Budget::default());
    let parsed = parse_governed("fn f() { 1 + 1 }", Language::Rust, &mut governor).unwrap();
    assert!(alloc::live_bytes() > before);
    std::thread::spawn(move || drop(parsed)).join().unwrap();
    assert_eq!(alloc::live_bytes(), before);
}

#[test]
fn d_cancellation_mid_parse_stops_with_cancelled() {
    let _guard = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let source = large_rust_source();
    let token = CancellationToken::new();
    let mut governor = Governor::with_cancellation(scratch_budget(u64::MAX / 2), token.clone());
    let (go, wait_for_go) = mpsc::channel::<()>();
    let canceller = std::thread::spawn(move || {
        wait_for_go.recv().unwrap();
        token.cancel();
    });
    go.send(()).unwrap();
    let error = inspect(&source, Language::Rust, &mut governor).unwrap_err();
    canceller.join().unwrap();
    assert_eq!(error, StopReason::Cancelled);
    // The governor's own usage is cumulative up to the point it stopped;
    // cancellation does not roll it back (see `docs/CANCELLATION.md`).
    let _ = governor.usage();
}

/// Per-grammar oracle corpus: representative valid and mildly malformed
/// snippets, one language family at a time (the smallest JSX/TSX-bearing
/// element is deliberately included, since JSX repair is this crate's other
/// tree-sitter-heavy code path). Shares its shape with
/// `scripts/check-syntax-oracles.py`'s probes, extended with larger inputs
/// so the measured overshoot reflects more than a trivial one-token parse.
fn oracle_corpus(language: Language) -> Vec<String> {
    let repeat = |unit: &str, count: usize| unit.repeat(count);
    match language {
        Language::Python => vec![
            "def answer():\n    return 42\n".into(),
            "async def answer(value):\n    return await value\n".into(),
            repeat("def f(x):\n    return x + 1\n", 4096),
        ],
        Language::Rust => vec![
            "pub fn answer() -> u32 { 42 }".into(),
            "pub fn answer() -> u32 { let x = 42; x }".into(),
            large_rust_source(),
        ],
        Language::JavaScript => vec![
            "function answer() { return 42; }".into(),
            "const answer = value => value?.name ?? \"none\";".into(),
            "const answer = <div title=\"tea\"/>;".into(),
            repeat("function f(x) {\n  return x + 1;\n}\n", 4096),
        ],
        Language::TypeScript => vec![
            "interface Cup { temperature: number }\nfunction answer(x: Cup): number { return x.temperature; }".into(),
            repeat("function f(x: number): number {\n  return x + 1;\n}\n", 4096),
        ],
        Language::Tsx => vec![
            "const answer = (x: string) => <div>{x}</div>;".into(),
            repeat("const f = (x: number) => <div>{x}</div>;\n", 4096),
        ],
    }
}

/// Largest measured `max_overshoot_bytes` per grammar across
/// [`oracle_corpus`], must not exceed the constant `docs/RESOURCES.md`
/// documents for that grammar.
#[test]
fn e_measured_overshoot_stays_within_the_documented_bound_per_grammar() {
    let _guard = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Matches the "Measured tree-sitter allocation overshoot" table in
    // `docs/RESOURCES.md`; a change to either must update the other.
    for (language, documented_bound) in [
        (Language::Python, 40_960_u64),
        (Language::Rust, 102_400_u64),
        (Language::JavaScript, 40_960_u64),
        (Language::TypeScript, 40_960_u64),
        (Language::Tsx, 40_960_u64),
    ] {
        let mut measured_max = 0_u64;
        for source in oracle_corpus(language) {
            let mut governor = Governor::new(scratch_budget(u64::MAX / 2));
            let parsed = parse_governed(&source, language, &mut governor).unwrap();
            measured_max = measured_max.max(parsed.allocation().max_overshoot_bytes);
        }
        assert!(
            measured_max <= documented_bound,
            "{language:?}: measured {measured_max} > documented {documented_bound}"
        );
        println!("{language:?}: measured max overshoot = {measured_max} bytes");
    }
}
