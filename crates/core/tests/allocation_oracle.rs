// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
#![allow(unsafe_code)]
//! Measured-allocation oracle: a test-only counting `#[global_allocator]`
//! (this file only; no dependencies) proves, for every governed public
//! operation, at two deterministically generated input scales (a base size
//! and a size at least 16x larger), that real allocator bytes never exceed
//!
//! ```text
//! measured_bytes_allocated_during_op ≤ governor.usage().total_scratch_charged + input_len + K
//! ```
//!
//! for one documented per-operation `K` (`docs/RESOURCES.md`) that stays the
//! *same constant* across both scales. `total_scratch_charged`
//! (`crates/core/src/contract.rs`) is the monotone sum of every scratch byte
//! this crate's governed engines have ever charged, never reduced by a
//! release; unlike `peak_scratch_bytes` (a high-water mark of *simultaneously*
//! live scratch), it also bounds the cumulative allocator churn of an
//! operation that charges and releases scratch many times in a loop (fuzzy
//! edit-distance scans, regex backtracking, per-candidate ranking), where the
//! allocator performs one real call per iteration even though live scratch
//! returns to a low water mark between iterations. Because both sides of the
//! inequality now scale together with input size, a *constant* `K` across
//! scales is a real falsifiability property: an uncharged allocation that
//! grows linearly (or worse) with input size, however small `K` was chosen,
//! is caught at the 16x scale even if it slips under a generously chosen `K`
//! at the base scale.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::Mutex;

use purrassist_core::{
    Budget, Governor, Outcome, edits,
    embeddings::{Embedding, EmbeddingIndex, EmbeddingPolicy},
    intent::{Concept, Domain, Rule},
    json,
    markdown::{Dialect, Document, ListItemEdit, TableRowEdit},
    matching::{self, Candidate, MatchRequest, SelectionPolicy},
    paths::PathIndex,
    pattern::Pattern,
    schema::{Schema, recovery::RecoveryRequest},
};

/// The "at least 16x larger" scale pair every workload is measured at.
const SCALES: [usize; 2] = [2, 32];

thread_local! {
    /// Bytes granted to *this thread*. Counting per thread keeps the test
    /// harness's own allocations on other threads (spawning the next test,
    /// capturing output) out of a measured window; a single process-global
    /// counter picked those up and made the bound depend on test ordering.
    static ALLOCATED: Cell<u64> = const { Cell::new(0) };
}

/// Add `bytes` to the calling thread's counter. Uses `try_with` because the
/// allocator can run while this thread's locals are being torn down, when the
/// counter is gone and the allocation belongs to no measured window.
fn count(bytes: usize) {
    let _ = ALLOCATED.try_with(|allocated| {
        allocated.set(allocated.get().saturating_add(bytes as u64));
    });
}

fn allocated() -> u64 {
    ALLOCATED.with(Cell::get)
}

struct CountingAllocator;

// SAFETY: every method delegates directly to `System` with the same layout
// (or the same pointer/layout/new-size triple for `realloc`), only adding a
// counter update around the call. It never inspects or mutates the returned
// memory and never changes the contract `GlobalAlloc` requires of `System`.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        // SAFETY: `layout` is passed through unchanged to the system allocator.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr`/`layout` are passed through unchanged; this method
        // only measures allocation growth, so deallocation is not counted.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        // SAFETY: `layout` is passed through unchanged to the system allocator.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size > layout.size() {
            count(new_size - layout.size());
        }
        // SAFETY: `ptr`, `layout`, and `new_size` are passed through unchanged
        // to the system allocator, which is the same contract this method must
        // itself uphold.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Serializes every measured test's *entire body* in this process. Counting is
/// per thread, so other threads cannot perturb a measurement; the lock also
/// keeps measured work from competing with concurrent tests for shared
/// process-wide state (lazy statics initialised by whichever test runs first).
static MEASURE_LOCK: Mutex<()> = Mutex::new(());

/// Acquire exclusive measurement for the rest of the calling test. Hold the
/// returned guard for the whole test body.
fn exclusive() -> std::sync::MutexGuard<'static, ()> {
    MEASURE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Run `op`, returning its result and the bytes the allocator granted while
/// it ran (allocation only; frees are not subtracted, since the bound is on
/// bytes *allocated during* the operation, not net/live bytes). Callers must
/// hold an [`exclusive`] guard for their whole test body. Only this thread's
/// allocations are counted.
///
/// `op` runs once, unmeasured, before the timed call. Every workload here
/// constructs a fresh [`Governor`] inside `op` itself, so a repeat call is
/// exactly representative, never cumulative. This warm-up absorbs one-time,
/// per-thread costs that are not this operation's own — lazy statics,
/// `std::fmt` machinery, and this test binary's first touch of a shared
/// runtime table — so the measured window reflects steady-state allocation,
/// not cold-start noise that would otherwise vary with `cargo test`'s thread
/// scheduling.
fn measure<T>(mut op: impl FnMut() -> T) -> (T, u64) {
    let _ = op();
    let before = allocated();
    let result = op();
    let after = allocated();
    (result, after.saturating_sub(before))
}

fn size(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// Assert the scale-invariant bound at every scale in [`SCALES`], with the
/// *same* `k` at each. `results[i]` is `(measured_bytes, total_scratch_charged,
/// input_len)` for `SCALES[i]`.
#[track_caller]
fn assert_scaled(operation: &str, k: u64, results: [(u64, u64, u64); SCALES.len()]) {
    for (scale, (bytes, total_charged, input_len)) in SCALES.into_iter().zip(results) {
        let bound = total_charged.saturating_add(input_len).saturating_add(k);
        assert!(
            bytes <= bound,
            "{operation} (scale {scale}): measured {bytes} bytes allocated > bound {bound} \
             (total_scratch_charged={total_charged}, input_len={input_len}, K={k})"
        );
    }
}

#[test]
fn oracle_schema_compile() {
    fn source(n: usize) -> String {
        let mut props = String::new();
        for i in 0..n {
            if i > 0 {
                props.push(',');
            }
            let _ = write!(props, r#""field{i}":{{"type":"string"}}"#);
        }
        format!(
            r#"{{"type":"object","properties":{{{props},"colour":{{"type":"string","pattern":"^[a-z]+$"}}}},"required":["colour"]}}"#
        )
    }
    fn run(n: usize) -> (u64, u64, u64) {
        let src = source(n);
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let schema = Schema::compile_governed(&src, &mut governor).unwrap();
            let _ = schema;
            governor.usage().total_scratch_charged
        });
        (bytes, charged, size(src.len()))
    }
    let _guard = exclusive();
    assert_scaled("schema_compile", 4096, [run(SCALES[0]), run(SCALES[1])]);
}

#[test]
fn oracle_schema_validate() {
    fn pair(n: usize) -> (String, String) {
        let mut schema_props = String::new();
        let mut instance_props = String::new();
        for i in 0..n {
            if i > 0 {
                schema_props.push(',');
                instance_props.push(',');
            }
            let _ = write!(schema_props, r#""field{i}":{{"type":"string"}}"#);
            let _ = write!(instance_props, r#""field{i}":"value{i}""#);
        }
        (
            format!(r#"{{"type":"object","properties":{{{schema_props}}}}}"#),
            format!(r"{{{instance_props}}}"),
        )
    }
    fn run(n: usize) -> (u64, u64, u64) {
        let (schema_source, instance_source) = pair(n);
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let schema = Schema::compile_governed(&schema_source, &mut governor).unwrap();
            let instance = json::parse_with(&instance_source, &mut governor).unwrap();
            let report = schema.validate_with(&instance, &mut governor).unwrap();
            assert!(report.valid);
            governor.usage().total_scratch_charged
        });
        (
            bytes,
            charged,
            size(schema_source.len().saturating_add(instance_source.len())),
        )
    }
    let _guard = exclusive();
    assert_scaled("schema_validate", 2048, [run(SCALES[0]), run(SCALES[1])]);
}

#[test]
fn oracle_recover() {
    // One malformed key alongside `n` already-valid padding properties: the
    // repair search has exactly one proposal site regardless of `n` (padding
    // never becomes a second search branch), so input size scales without
    // multiplying the candidate search itself past the default budget.
    fn source(n: usize) -> String {
        let mut padding = String::new();
        for i in 0..n {
            let _ = write!(padding, r#","padding{i}":"value{i}""#);
        }
        format!(r#"{{color: "red"{padding},}}"#)
    }
    const SCHEMA: &str =
        r#"{"type":"object","properties":{"colour":{"type":"string"}},"required":["colour"]}"#;
    fn run(n: usize) -> (u64, u64, u64) {
        let input = source(n);
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let schema = Schema::compile_governed(SCHEMA, &mut governor).unwrap();
            let result = schema.recover_governed(&input, RecoveryRequest::default(), &mut governor);
            assert!(matches!(
                result.outcome,
                Outcome::Resolved(_) | Outcome::BestGuess(_) | Outcome::Ambiguous(_)
            ));
            result.receipt.usage.total_scratch_charged
        });
        (bytes, charged, size(input.len()))
    }
    let _guard = exclusive();
    assert_scaled("recover", 8192, [run(SCALES[0]), run(SCALES[1])]);
}

#[test]
fn oracle_recover_composed_ambiguous() {
    fn source(n: usize) -> String {
        let mut items = String::from(r#"{"x":1,"x":2}"#);
        for i in 1..n {
            let _ = write!(items, r#",{{"padding":"item{i}"}}"#);
        }
        format!("[{items}]")
    }
    const SCHEMA: &str = r#"{"type":"array","items":{"type":"object"}}"#;
    fn run(n: usize) -> (u64, u64, u64) {
        let input = source(n);
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let schema = Schema::compile_governed(SCHEMA, &mut governor).unwrap();
            let result = schema.recover_governed(&input, RecoveryRequest::default(), &mut governor);
            assert!(matches!(result.outcome, Outcome::Ambiguous(_)));
            result.receipt.usage.total_scratch_charged
        });
        (bytes, charged, size(input.len()))
    }
    let _guard = exclusive();
    assert_scaled(
        "recover_composed_ambiguous",
        4096,
        [run(SCALES[0]), run(SCALES[1])],
    );
}

#[test]
fn oracle_json_parse() {
    fn source(n: usize) -> String {
        let mut pairs = String::new();
        for i in 0..n {
            if i > 0 {
                pairs.push(',');
            }
            let _ = write!(pairs, r#""key{i}":"value{i}""#);
        }
        format!(r#"{{"tea":["hot","iced"],{pairs}}}"#)
    }
    fn run(n: usize) -> (u64, u64, u64) {
        let input = source(n);
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let node = json::parse_with(&input, &mut governor).unwrap();
            assert!(node.get("tea").is_some());
            governor.usage().total_scratch_charged
        });
        (bytes, charged, size(input.len()))
    }
    let _guard = exclusive();
    assert_scaled("json_parse", 1024, [run(SCALES[0]), run(SCALES[1])]);
}

#[test]
fn oracle_json_recover_syntax() {
    fn source(n: usize) -> String {
        let mut pairs = String::from("tea: 'hot'");
        for i in 0..n {
            let _ = write!(pairs, ", count{i}: {i}");
        }
        format!("{{{pairs},}}")
    }
    fn run(n: usize) -> (u64, u64, u64) {
        let input = source(n);
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let schema = Schema::compile_governed("true", &mut governor).unwrap();
            let result = schema.recover_governed(&input, RecoveryRequest::default(), &mut governor);
            assert!(matches!(
                result.outcome,
                Outcome::Resolved(_) | Outcome::BestGuess(_) | Outcome::Ambiguous(_)
            ));
            result.receipt.usage.total_scratch_charged
        });
        (bytes, charged, size(input.len()))
    }
    let _guard = exclusive();
    assert_scaled(
        "json_recover_syntax",
        2048,
        [run(SCALES[0]), run(SCALES[1])],
    );
}

#[test]
fn oracle_pattern_compile() {
    fn source(n: usize) -> String {
        format!("^{}$", "[A-Za-z]".repeat(n.max(1)))
    }
    fn run(n: usize) -> (u64, u64, u64) {
        let src = source(n);
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let pattern = Pattern::compile_governed(&src, &mut governor).unwrap();
            let _ = pattern;
            governor.usage().total_scratch_charged
        });
        (bytes, charged, size(src.len()))
    }
    let _guard = exclusive();
    assert_scaled("pattern_compile", 2048, [run(SCALES[0]), run(SCALES[1])]);
}

#[test]
fn oracle_pattern_match() {
    const SOURCE: &str = "^[A-Za-z_]+$";
    fn text(n: usize) -> String {
        "identifier_".repeat(n.max(1))
    }
    fn run(n: usize) -> (u64, u64, u64) {
        let haystack = text(n);
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let pattern = Pattern::compile_governed(SOURCE, &mut governor).unwrap();
            let matched = pattern.is_match(&haystack, &mut governor).unwrap();
            assert!(matched);
            governor.usage().total_scratch_charged
        });
        (bytes, charged, size(haystack.len()))
    }
    let _guard = exclusive();
    assert_scaled("pattern_match", 8192, [run(SCALES[0]), run(SCALES[1])]);
}

fn markdown_render_source(n: usize) -> String {
    let mut out = String::from("# Tea\n\n");
    for i in 0..n.max(1) {
        let _ = writeln!(out, "- item{i}");
    }
    out.push('\n');
    out.push_str("| kind | temp |\n| --- | --- |\n");
    for i in 0..n.max(1) {
        let _ = writeln!(out, "| black{i} | hot |");
    }
    out.push('\n');
    for i in 0..n.max(1) {
        let _ = writeln!(out, "[black{i}]: /black{i}");
    }
    out
}

#[test]
fn oracle_markdown_render() {
    fn run(n: usize) -> (u64, u64, u64) {
        let source = markdown_render_source(n);
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            let html = document.render_html(Dialect::Gfm, &mut governor).unwrap();
            assert!(html.contains("Tea"));
            governor.usage().total_scratch_charged
        });
        (bytes, charged, size(source.len()))
    }
    let _guard = exclusive();
    assert_scaled("markdown_render", 4096, [run(SCALES[0]), run(SCALES[1])]);
}

#[test]
fn oracle_markdown_plan_section() {
    fn source(n: usize) -> String {
        let mut out = String::new();
        for i in 0..n.max(1) {
            let _ = writeln!(out, "# Section{i}\nBody{i}.");
        }
        out
    }
    fn run(n: usize) -> (u64, u64, u64) {
        let source = source(n);
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            let plan = document
                .replace_section_governed(0, "Cold.\n", &mut governor)
                .unwrap();
            let _ = plan;
            governor.usage().total_scratch_charged
        });
        (bytes, charged, size(source.len()))
    }
    let _guard = exclusive();
    assert_scaled(
        "markdown_plan_section",
        4096,
        [run(SCALES[0]), run(SCALES[1])],
    );
}

#[test]
fn oracle_markdown_plan_list() {
    fn source(n: usize) -> String {
        let mut out = String::new();
        for i in 0..n.max(1) {
            let _ = writeln!(out, "- item{i}");
        }
        out
    }
    fn run(n: usize) -> (u64, u64, u64) {
        let source = source(n);
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            let plan = document
                .edit_list_item(
                    &[0],
                    &ListItemEdit::After {
                        marker: "-".into(),
                        body: "iced".into(),
                    },
                    &mut governor,
                )
                .unwrap();
            let _ = plan;
            governor.usage().total_scratch_charged
        });
        (bytes, charged, size(source.len()))
    }
    let _guard = exclusive();
    assert_scaled("markdown_plan_list", 4096, [run(SCALES[0]), run(SCALES[1])]);
}

fn markdown_table_source(n: usize) -> String {
    let mut out = String::from("| Drink | Temp |\n| --- | --- |\n");
    for i in 0..n.max(1) {
        let _ = writeln!(out, "| item{i} | hot |");
    }
    out
}

#[test]
fn oracle_markdown_plan_table_row() {
    fn run(n: usize) -> (u64, u64, u64) {
        let source = markdown_table_source(n);
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            let edit = TableRowEdit::Insert {
                index: 0,
                cells: vec!["iced tea".into(), "cold".into()],
            };
            let plan = document.edit_table_row(&[0], &edit, &mut governor).unwrap();
            let _ = plan;
            governor.usage().total_scratch_charged
        });
        (bytes, charged, size(source.len()))
    }
    let _guard = exclusive();
    assert_scaled(
        "markdown_plan_table_row",
        4096,
        [run(SCALES[0]), run(SCALES[1])],
    );
}

#[test]
fn oracle_markdown_plan_table_cell() {
    fn run(n: usize) -> (u64, u64, u64) {
        let source = markdown_table_source(n);
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            let plan = document
                .replace_table_cell(&[0], 1, 0, "**iced tea**", &mut governor)
                .unwrap();
            let _ = plan;
            governor.usage().total_scratch_charged
        });
        (bytes, charged, size(source.len()))
    }
    let _guard = exclusive();
    assert_scaled(
        "markdown_plan_table_cell",
        4096,
        [run(SCALES[0]), run(SCALES[1])],
    );
}

#[test]
fn oracle_markdown_plan_reference() {
    fn source(n: usize) -> String {
        let mut out = String::new();
        for i in 0..n.max(1) {
            let _ = writeln!(out, "[tea{i}]: /hot{i}");
        }
        out
    }
    fn run(n: usize) -> (u64, u64, u64) {
        let source = source(n);
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let document = Document::parse(&source, &mut governor).unwrap();
            let plan = document
                .replace_reference(&[0], 0, "/iced", Some("Tea"), &mut governor)
                .unwrap();
            let _ = plan;
            governor.usage().total_scratch_charged
        });
        (bytes, charged, size(source.len()))
    }
    let _guard = exclusive();
    assert_scaled(
        "markdown_plan_reference",
        4096,
        [run(SCALES[0]), run(SCALES[1])],
    );
}

fn ranking_candidates(n: usize) -> Vec<Candidate> {
    let mut candidates = vec![Candidate::new("tea", "tea")];
    for i in 0..n {
        candidates.push(Candidate::new(
            format!("other{i}"),
            format!("other-drink-{i}"),
        ));
    }
    candidates
}

#[test]
fn oracle_matching_rank() {
    fn run(n: usize) -> (u64, u64, u64) {
        let candidates = ranking_candidates(n);
        let input_len = size(candidates.iter().map(|c| c.text.len()).sum());
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let result = matching::rank_governed(
                "tea",
                &candidates,
                &MatchRequest::default(),
                &mut governor,
            );
            assert!(matches!(result.outcome, Outcome::Resolved(_)));
            result.receipt.usage.total_scratch_charged
        });
        (bytes, charged, input_len)
    }
    let _guard = exclusive();
    assert_scaled("matching_rank", 8192, [run(SCALES[0]), run(SCALES[1])]);
}

#[test]
fn oracle_matching_resolve() {
    fn run(n: usize) -> (u64, u64, u64) {
        let candidates = ranking_candidates(n);
        let input_len = size(candidates.iter().map(|c| c.text.len()).sum());
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let result = matching::resolve_governed(
                "tea",
                &candidates,
                &MatchRequest::default(),
                SelectionPolicy::default(),
                &mut governor,
            );
            assert!(matches!(result.outcome, Outcome::Resolved(_)));
            result.receipt.usage.total_scratch_charged
        });
        (bytes, charged, input_len)
    }
    let _guard = exclusive();
    assert_scaled("matching_resolve", 8192, [run(SCALES[0]), run(SCALES[1])]);
}

#[test]
fn oracle_paths() {
    fn list(n: usize) -> Vec<String> {
        (0..n.max(1)).map(|i| format!("src/file{i}.rs")).collect()
    }
    fn run(n: usize) -> (u64, u64, u64) {
        let paths = list(n);
        let input_len = size(paths.iter().map(String::len).sum());
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let index = PathIndex::new(paths.clone(), &mut governor).unwrap();
            let answer = index.resolve_governed(
                "src/file0.rs",
                "",
                &MatchRequest::default(),
                purrassist_core::paths::SelectionPolicy::default(),
                &mut governor,
            );
            assert!(matches!(answer.outcome, Outcome::Resolved(_)));
            answer.receipt.usage.total_scratch_charged
        });
        (bytes, charged, input_len)
    }
    let _guard = exclusive();
    assert_scaled("paths", 4096, [run(SCALES[0]), run(SCALES[1])]);
}

fn padded_source(n: usize) -> String {
    let mut source = String::new();
    for i in 0..n.max(1) {
        let _ = write!(source, "padding{i} ");
    }
    source.push_str("hello world");
    source
}

#[test]
fn oracle_edits_exact() {
    fn run(n: usize) -> (u64, u64, u64) {
        let source = padded_source(n);
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let plan = edits::locate_governed(&source, "world", "earth", &mut governor);
            assert!(matches!(plan.outcome, Outcome::Resolved(_)));
            plan.receipt.usage.total_scratch_charged
        });
        (bytes, charged, size(source.len()))
    }
    let _guard = exclusive();
    assert_scaled("edits_exact", 2048, [run(SCALES[0]), run(SCALES[1])]);
}

#[test]
fn oracle_edits_fuzzy() {
    fn run(n: usize) -> (u64, u64, u64) {
        let source = padded_source(n);
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let plan = edits::locate_fuzzy_governed(&source, "wurld", "earth", 1, &mut governor);
            assert!(matches!(plan.outcome, Outcome::BestGuess(_)));
            plan.receipt.usage.total_scratch_charged
        });
        (bytes, charged, size(source.len()))
    }
    let _guard = exclusive();
    assert_scaled("edits_fuzzy", 8192, [run(SCALES[0]), run(SCALES[1])]);
}

#[test]
fn oracle_edits_unified() {
    fn pair(n: usize) -> (String, String) {
        let padding = n.max(1);
        let mut source = String::new();
        for i in 0..padding {
            let _ = writeln!(source, "line{i}");
        }
        source.push_str("one\ntwo\n");
        let start = padding + 1;
        let patch = format!(
            "--- a/example\n+++ b/example\n@@ -{start},2 +{start},2 @@\n one\n-two\n+three\n"
        );
        (source, patch)
    }
    fn run(n: usize) -> (u64, u64, u64) {
        let (source, patch) = pair(n);
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let plan = edits::plan_unified_governed(&source, &patch, &mut governor);
            assert!(matches!(plan.outcome, Outcome::Resolved(_)));
            plan.receipt.usage.total_scratch_charged
        });
        (
            bytes,
            charged,
            size(source.len().saturating_add(patch.len())),
        )
    }
    let _guard = exclusive();
    assert_scaled("edits_unified", 2048, [run(SCALES[0]), run(SCALES[1])]);
}

fn embedding_candidates(n: usize) -> Vec<Embedding> {
    let mut candidates = vec![Embedding {
        id: "tea".into(),
        vector: vec![1.0, 0.0],
    }];
    for i in 0..n {
        candidates.push(Embedding {
            id: format!("other{i}"),
            vector: vec![0.0, 1.0],
        });
    }
    candidates
}

#[test]
fn oracle_embeddings() {
    fn run(n: usize) -> (u64, u64, u64) {
        let candidates = embedding_candidates(n);
        let input_len = size(candidates.iter().map(|c| c.id.len()).sum());
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let index = EmbeddingIndex::new("caller-space-v1", &candidates, &mut governor).unwrap();
            let outcome = index
                .select(&[1.0, 0.0], EmbeddingPolicy::default(), &mut governor)
                .unwrap();
            assert!(matches!(outcome, Outcome::BestGuess(_)));
            governor.usage().total_scratch_charged
        });
        (bytes, charged, input_len)
    }
    let _guard = exclusive();
    assert_scaled("embeddings", 2048, [run(SCALES[0]), run(SCALES[1])]);
}

#[test]
fn oracle_intent() {
    fn domain_and_concepts(n: usize) -> (Domain, Vec<Concept>) {
        let mut attributes = BTreeMap::new();
        attributes.insert("drink".to_string(), "tea".to_string());
        let mut rules = vec![Rule {
            id: "tea".into(),
            phrase: "tea".into(),
            assignments: attributes.clone(),
        }];
        for i in 0..n {
            let mut other = BTreeMap::new();
            other.insert("other".to_string(), format!("value{i}"));
            rules.push(Rule {
                id: format!("other{i}"),
                phrase: format!("other{i}"),
                assignments: other,
            });
        }
        let domain = Domain {
            id: "oracle.drinks.v1".into(),
            rules,
            ignored_words: BTreeSet::default(),
        };
        let concepts = vec![Concept {
            id: "tea".into(),
            label: "tea".into(),
            attributes,
        }];
        (domain, concepts)
    }
    fn run(n: usize) -> (u64, u64, u64) {
        let (domain, concepts) = domain_and_concepts(n);
        let input_len = size(domain.rules.iter().map(|rule| rule.phrase.len()).sum());
        let (charged, bytes) = measure(|| {
            let mut governor = Governor::new(Budget::default());
            let result = domain.resolve_governed("tea", &concepts, &mut governor);
            assert!(matches!(result.outcome, Outcome::Resolved(_)));
            result.receipt.usage.total_scratch_charged
        });
        (bytes, charged, input_len)
    }
    let _guard = exclusive();
    assert_scaled("intent", 2048, [run(SCALES[0]), run(SCALES[1])]);
}
