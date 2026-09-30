// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
#![allow(unsafe_code)]
//! Per-workload latency (median/p95 nanoseconds), real allocation call
//! count, and real peak live-heap bytes, for every domain Task 13 of
//! `docs/qualification`'s plan enumerates: Markdown rendering and every
//! Task 9 structural operation, exact/fuzzy/unified edits, matching
//! rank/resolve, paths, pattern compile/match (both the derivative and
//! backtracking engines), and schema compile/validate/recover.
//!
//! The counting `#[global_allocator]` below is this bench binary's own
//! (this file only; no dependencies; a from-scratch instrument distinct
//! from `crates/core/tests/allocation_oracle.rs`'s, which measures total
//! bytes granted rather than peak live bytes), consistent with this
//! project's existing pattern of a test/bench-scoped counting allocator.
//! Output is a JSON array printed to stdout; `scripts/measure-resources.py`
//! captures it verbatim into `docs/qualification/resources-linux-x86_64.json`.
use purrassist_core::{
    Budget, Governor, Outcome,
    edits::{self, EditPlan},
    markdown::{Dialect, Document, ListItemEdit, SectionInsertion, TableColumnEdit, TableRowEdit},
    matching::{self, Candidate, MatchRequest},
    paths::PathIndex,
    pattern::Pattern,
    schema::{Schema, recovery::RecoveryRequest},
    wire::{array, integer, object, string},
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;
use std::time::Instant;

thread_local! {
    static LIVE_BYTES: Cell<u64> = const { Cell::new(0) };
    static PEAK_BYTES: Cell<u64> = const { Cell::new(0) };
    static ALLOCATION_CALLS: Cell<u64> = const { Cell::new(0) };
}

fn on_alloc(bytes: usize) {
    let _ = LIVE_BYTES.try_with(|live| {
        let value = live.get().saturating_add(bytes as u64);
        live.set(value);
        let _ = PEAK_BYTES.try_with(|peak| peak.set(peak.get().max(value)));
    });
    let _ = ALLOCATION_CALLS.try_with(|calls| calls.set(calls.get() + 1));
}
fn on_dealloc(bytes: usize) {
    let _ = LIVE_BYTES.try_with(|live| live.set(live.get().saturating_sub(bytes as u64)));
}
fn reset_counters() {
    let _ = LIVE_BYTES.try_with(|live| {
        let _ = PEAK_BYTES.try_with(|peak| peak.set(live.get()));
    });
    let _ = ALLOCATION_CALLS.try_with(|cell| cell.set(0));
}

struct CountingAllocator;

// SAFETY: every method delegates directly to `System` with the same layout
// (or the same pointer/layout/new-size triple for `realloc`), only adding a
// counter update around the call; it never inspects or mutates the returned
// memory and never changes the contract `GlobalAlloc` requires of `System`.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: `layout` is passed through unchanged to the system allocator.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            on_alloc(layout.size());
        }
        pointer
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        on_dealloc(layout.size());
        // SAFETY: `ptr`/`layout` are passed through unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: `layout` is passed through unchanged to the system allocator.
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            on_alloc(layout.size());
        }
        pointer
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: pointer, layout, and new_size are forwarded unchanged.
        let pointer = unsafe { System.realloc(ptr, layout, new_size) };
        if !pointer.is_null() {
            on_dealloc(layout.size());
            on_alloc(new_size);
        }
        pointer
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn process_peak_rss_bytes() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").expect("Linux procfs RSS");
    let value = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .expect("VmHWM");
    let mut fields = value.split_whitespace();
    let kib = fields
        .next()
        .expect("VmHWM value")
        .parse::<u64>()
        .expect("RSS integer");
    assert_eq!(fields.next(), Some("kB"));
    assert!(fields.next().is_none());
    kib.checked_mul(1024).expect("RSS bytes")
}

const SAMPLES: usize = 200;

fn measure(name: &str, op: &dyn Fn()) -> purrassist_core::json::Node {
    // Unmeasured warm-up: absorbs one-time per-process costs (lazy statics,
    // first-touch allocator pages) that are not this operation's own.
    op();
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        op();
        samples.push(u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX));
    }
    samples.sort_unstable();
    reset_counters();
    let baseline = LIVE_BYTES.with(Cell::get);
    op();
    let allocation_calls = ALLOCATION_CALLS.with(Cell::get);
    let peak_bytes = PEAK_BYTES.with(Cell::get).saturating_sub(baseline);
    object([
        ("case", string(name)),
        (
            "samples",
            integer(u64::try_from(SAMPLES).unwrap_or(u64::MAX)),
        ),
        ("median_ns", integer(samples[SAMPLES / 2])),
        ("p95_ns", integer(samples[SAMPLES * 95 / 100])),
        ("allocation_calls", integer(allocation_calls)),
        ("peak_live_bytes", integer(peak_bytes)),
        ("process_peak_rss_bytes", integer(process_peak_rss_bytes())),
    ])
}

const SCHEMA: &str = r#"{"type":"object","required":["temperature"],"properties":{"temperature":{"type":"number"}},"additionalProperties":false}"#;
const MARKDOWN_SOURCE: &str = "# Tea\n\nSome tea.\n\n## Coffee\n\nSome coffee.\n\n- one\n- two\n- three\n\n1. first\n2. second\n\n| A | B |\n| --- | --- |\n| tea | cold |\n| coffee | hot |\n\n[link][ref]\n\n[ref]: /destination \"title\"\n";

/// One named workload closure; boxed and collected into `workloads()` below.
type Workload = (&'static str, Box<dyn Fn()>);

/// A flat registry of every measured workload; naturally long (one
/// three-to-six-line entry per named workload, mirroring `CASES`-style
/// tables elsewhere in this crate), so splitting it into more functions
/// would scatter the registry without shortening any single decision.
#[allow(clippy::too_many_lines)]
fn workloads() -> Vec<Workload> {
    vec![
        (
            "schema_compile",
            Box::new(|| {
                black_box(Schema::compile(SCHEMA).expect("schema"));
            }),
        ),
        (
            "schema_validate",
            Box::new(|| {
                let schema = Schema::compile(SCHEMA).expect("schema");
                let instance = purrassist_core::json::parse(r#"{"temperature":21.5}"#).unwrap();
                let validation = schema.validate(&instance).unwrap();
                assert!(validation.valid);
                black_box(validation);
            }),
        ),
        (
            "schema_recover",
            Box::new(|| {
                let schema = Schema::compile(SCHEMA).expect("schema");
                let result = schema.recover(
                    "```json\n[[{temperatur: '42',}]]\n```",
                    RecoveryRequest::default(),
                );
                let Outcome::Resolved(value) = &result.outcome else {
                    panic!("composed recovery workload must resolve");
                };
                assert_eq!(value.to_json(), r#"{"temperature":42}"#);
                black_box(result);
            }),
        ),
        (
            "matching_rank",
            Box::new(|| {
                let candidates: Vec<_> = (0..32)
                    .map(|index| {
                        Candidate::new(index.to_string(), format!("document_{index}.json"))
                    })
                    .collect();
                let result =
                    matching::rank("document_16.json", &candidates, &MatchRequest::default());
                let Outcome::Resolved(ranked) = &result.outcome else {
                    panic!("ranking workload must complete");
                };
                assert_eq!(ranked.first().expect("ranked candidate").candidate.id, "16");
                black_box(result);
            }),
        ),
        (
            "matching_resolve",
            Box::new(|| {
                let candidates: Vec<_> = (0..32)
                    .map(|index| {
                        Candidate::new(index.to_string(), format!("document_{index}.json"))
                    })
                    .collect();
                let result = matching::resolve(
                    "document_16.json",
                    &candidates,
                    &MatchRequest::default(),
                    matching::SelectionPolicy::default(),
                );
                let Outcome::Resolved(selected) = &result.outcome else {
                    panic!("exact matching workload must resolve");
                };
                assert_eq!(selected.candidate.id, "16");
                black_box(result);
            }),
        ),
        (
            "paths_resolve",
            Box::new(|| {
                let mut governor = Governor::new(Budget::default());
                let index = PathIndex::new(
                    (0..32).map(|index| format!("src/document_{index}.rs")),
                    &mut governor,
                )
                .expect("path index");
                let result = index.resolve("document_16.rs", "src", &MatchRequest::default());
                let (Outcome::Resolved(selected) | Outcome::BestGuess(selected)) = &result.outcome
                else {
                    panic!("contextual path workload must select one path");
                };
                assert_eq!(selected.path, "src/document_16.rs");
                black_box(result);
            }),
        ),
        (
            "pattern_compile",
            Box::new(|| {
                black_box(
                    Pattern::compile(r"^[A-Za-z][A-Za-z0-9_]{2,30}@[a-z0-9.-]+$").expect("pattern"),
                );
            }),
        ),
        (
            "pattern_match_derivative",
            Box::new(|| {
                let pattern =
                    Pattern::compile(r"^[A-Za-z][A-Za-z0-9_]{2,30}@[a-z0-9.-]+$").expect("pattern");
                let mut governor = Governor::new(Budget::default());
                assert!(black_box(
                    pattern
                        .is_match("reader_one@example.com", &mut governor)
                        .unwrap(),
                ));
            }),
        ),
        (
            "pattern_match_backtracking",
            Box::new(|| {
                let pattern =
                    Pattern::compile(r"^[A-Za-z][A-Za-z0-9_]{2,30}@[a-z0-9.-]+$").expect("pattern");
                let mut governor = Governor::new(Budget::default());
                assert!(black_box(
                    pattern
                        .is_match_backtracking("reader_one@example.com", &mut governor)
                        .unwrap(),
                ));
            }),
        ),
        (
            "edits_exact",
            Box::new(|| {
                let source = format!(
                    "{}needle{}",
                    "padding ".repeat(32),
                    " more padding".repeat(32)
                );
                let result = edits::locate(&source, "needle", "replacement", Budget::default());
                assert!(matches!(result.outcome, Outcome::Resolved(_)));
                black_box(result);
            }),
        ),
        (
            "edits_fuzzy",
            Box::new(|| {
                let source = format!(
                    "{}needle{}",
                    "padding ".repeat(32),
                    " more padding".repeat(32)
                );
                let result =
                    edits::locate_fuzzy(&source, "neelde", "replacement", 2, Budget::default());
                let Outcome::Ambiguous(plans) = &result.outcome else {
                    panic!("fuzzy workload must preserve equally close regions");
                };
                assert!(plans.len() > 1);
                let start = source.find("needle").expect("source needle");
                assert!(plans.iter().any(|plan| plan.edits.iter().any(|edit| {
                    edit.range == (start..start + "needle".len())
                        && edit.replacement == "replacement"
                })));
                black_box(result);
            }),
        ),
        (
            "edits_unified",
            Box::new(|| {
                let source = format!("{}one\ntwo\n", "padding line\n".repeat(32));
                let patch = format!(
                    "--- a/example\n+++ b/example\n@@ -{},2 +{},2 @@\n one\n-two\n+three\n",
                    33, 33
                );
                let result = edits::plan_unified(&source, &patch, Budget::default());
                assert!(matches!(result.outcome, Outcome::Resolved(_)));
                black_box(result);
            }),
        ),
        (
            "markdown_render",
            Box::new(|| {
                let mut governor = Governor::new(Budget::default());
                let doc = Document::parse(MARKDOWN_SOURCE, &mut governor).expect("markdown");
                black_box(doc.render_html(Dialect::Gfm, &mut governor).expect("html"));
            }),
        ),
        (
            "markdown_insert_section",
            Box::new(|| {
                let mut governor = Governor::new(Budget::default());
                let doc = Document::parse(MARKDOWN_SOURCE, &mut governor).expect("markdown");
                let plan: EditPlan = doc
                    .insert_section(SectionInsertion::Start, "# Water\nStill.\n", &mut governor)
                    .expect("insert section");
                black_box(plan);
            }),
        ),
        (
            "markdown_remove_section",
            Box::new(|| {
                let mut governor = Governor::new(Budget::default());
                let doc = Document::parse(MARKDOWN_SOURCE, &mut governor).expect("markdown");
                black_box(
                    doc.remove_section(4, &mut governor)
                        .expect("remove section"),
                );
            }),
        ),
        (
            "markdown_move_section",
            Box::new(|| {
                let mut governor = Governor::new(Budget::default());
                let doc = Document::parse(MARKDOWN_SOURCE, &mut governor).expect("markdown");
                black_box(
                    doc.move_section(4, SectionInsertion::Start, None, &mut governor)
                        .expect("move section"),
                );
            }),
        ),
        (
            "markdown_rename_heading",
            Box::new(|| {
                let mut governor = Governor::new(Budget::default());
                let doc = Document::parse(MARKDOWN_SOURCE, &mut governor).expect("markdown");
                black_box(
                    doc.rename_heading(0, "Iced Tea", &mut governor)
                        .expect("rename heading"),
                );
            }),
        ),
        (
            "markdown_list_edit",
            Box::new(|| {
                let mut governor = Governor::new(Budget::default());
                let doc = Document::parse(MARKDOWN_SOURCE, &mut governor).expect("markdown");
                black_box(
                    doc.edit_list_item(
                        &[8],
                        &ListItemEdit::After {
                            marker: "-".into(),
                            body: "four".into(),
                        },
                        &mut governor,
                    )
                    .expect("list edit"),
                );
            }),
        ),
        (
            "markdown_list_renumber",
            Box::new(|| {
                let mut governor = Governor::new(Budget::default());
                let doc = Document::parse(MARKDOWN_SOURCE, &mut governor).expect("markdown");
                black_box(
                    doc.renumber_list(&[12], 5, &mut governor)
                        .expect("renumber"),
                );
            }),
        ),
        (
            "markdown_table_row_move",
            Box::new(|| {
                let mut governor = Governor::new(Budget::default());
                let doc = Document::parse(MARKDOWN_SOURCE, &mut governor).expect("markdown");
                black_box(
                    doc.edit_table_row(
                        &[15],
                        &TableRowEdit::Move { index: 0, to: 1 },
                        &mut governor,
                    )
                    .expect("table row move"),
                );
            }),
        ),
        (
            "markdown_table_column_move",
            Box::new(|| {
                let mut governor = Governor::new(Budget::default());
                let doc = Document::parse(MARKDOWN_SOURCE, &mut governor).expect("markdown");
                black_box(
                    doc.edit_table_column(
                        &[15],
                        &TableColumnEdit::Move { index: 0, to: 1 },
                        &mut governor,
                    )
                    .expect("table column move"),
                );
            }),
        ),
        (
            "markdown_reference_add",
            Box::new(|| {
                let mut governor = Governor::new(Budget::default());
                let doc = Document::parse(MARKDOWN_SOURCE, &mut governor).expect("markdown");
                black_box(
                    doc.add_reference("other", "/elsewhere", None, &mut governor)
                        .expect("add reference"),
                );
            }),
        ),
        (
            "markdown_reference_relabel",
            Box::new(|| {
                let mut governor = Governor::new(Budget::default());
                let doc = Document::parse(MARKDOWN_SOURCE, &mut governor).expect("markdown");
                let site = doc.reference_sites(&mut governor).expect("sites")[0].clone();
                black_box(
                    doc.relabel_reference(&site.path, site.occurrence, "newref", &mut governor)
                        .expect("relabel"),
                );
            }),
        ),
        (
            "markdown_reference_remove",
            Box::new(|| {
                let mut governor = Governor::new(Budget::default());
                let doc = Document::parse(MARKDOWN_SOURCE, &mut governor).expect("markdown");
                let site = doc.reference_sites(&mut governor).expect("sites")[0].clone();
                black_box(
                    doc.remove_reference(&site.path, site.occurrence, &mut governor)
                        .expect("remove reference"),
                );
            }),
        ),
        (
            "markdown_reference_unlink",
            Box::new(|| {
                let mut governor = Governor::new(Budget::default());
                let doc = Document::parse(MARKDOWN_SOURCE, &mut governor).expect("markdown");
                let site = doc.reference_sites(&mut governor).expect("sites")[0].clone();
                black_box(
                    doc.unlink_reference(&site.path, site.occurrence, &mut governor)
                        .expect("unlink reference"),
                );
            }),
        ),
    ]
}

fn main() {
    let workloads = workloads();
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--list") => {
            for (name, _) in &workloads {
                println!("{name}");
            }
        }
        Some("--bench") | None => {
            let results: Vec<_> = workloads
                .iter()
                .map(|(name, op)| measure(name, op.as_ref()))
                .collect();
            println!("{}", array(results).to_json());
        }
        Some(selected) => {
            let (name, op) = workloads
                .iter()
                .find(|(name, _)| *name == selected)
                .unwrap_or_else(|| panic!("unknown latency workload: {selected}"));
            println!("{}", measure(name, op.as_ref()).to_json());
        }
    }
}
