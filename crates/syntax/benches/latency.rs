// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
#![allow(unsafe_code)]
//! Per-grammar `inspect` latency (median/p95 nanoseconds) and allocation
//! report (retained/peak/max-overshoot bytes, from this crate's own
//! `crates/syntax/src/alloc.rs` tree-sitter allocator accounting, not a
//! generic counting allocator) for every pinned grammar. Output is a JSON
//! array printed to stdout; `scripts/measure-resources.py` captures it
//! verbatim into `docs/qualification/resources-linux-x86_64.json`.
use purrassist_core::{
    Budget, Governor,
    wire::{array, integer, object, string},
};
use purrassist_syntax::{Language, inspect_with_allocation};
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

const fn name_for(language: Language) -> &'static str {
    match language {
        Language::Python => "python",
        Language::Rust => "rust",
        Language::JavaScript => "javascript",
        Language::TypeScript => "typescript",
        Language::Tsx => "tsx",
    }
}

fn source_for(language: Language) -> String {
    match language {
        Language::Python => {
            "def add(a, b):\n    total = a + b\n    if total > 0:\n        return total\n    return -total\n\nclass Widget:\n    def __init__(self, name):\n        self.name = name\n"
                .repeat(8)
        }
        Language::Rust => {
            "fn add(a: i32, b: i32) -> i32 {\n    let total = a + b;\n    if total > 0 { total } else { -total }\n}\nstruct Widget { name: String }\n"
                .repeat(8)
        }
        Language::JavaScript => {
            "function add(a, b) {\n  const total = a + b;\n  if (total > 0) { return total; }\n  return -total;\n}\nconst widget = <Widget name=\"tea\" />;\n"
                .repeat(8)
        }
        Language::TypeScript => {
            "function add(a: number, b: number): number {\n  const total: number = a + b;\n  if (total > 0) { return total; }\n  return -total;\n}\ninterface Widget { name: string }\n"
                .repeat(8)
        }
        Language::Tsx => {
            "function Widget(props: { name: string }): JSX.Element {\n  const total = props.name.length;\n  return <div>{total > 0 ? props.name : \"none\"}</div>;\n}\n"
                .repeat(8)
        }
    }
}

fn measure(language: Language) -> purrassist_core::json::Node {
    let source = source_for(language);
    let op = || {
        let mut governor = Governor::new(Budget::default());
        inspect_with_allocation(&source, language, &mut governor).expect("inspect")
    };
    // Unmeasured warm-up.
    black_box(op());
    let mut samples = Vec::with_capacity(SAMPLES);
    let mut tree_allocation_calls = 0;
    let mut retained_bytes = 0;
    let mut peak_bytes = 0;
    let mut max_overshoot_bytes = 0;
    for _ in 0..SAMPLES {
        let start = Instant::now();
        let (report, allocation) = op();
        assert!(report.valid, "valid syntax workload");
        drop(report);
        samples.push(u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX));
        tree_allocation_calls = tree_allocation_calls.max(allocation.allocation_calls);
        retained_bytes = retained_bytes.max(allocation.retained_bytes);
        peak_bytes = peak_bytes.max(allocation.peak_bytes);
        max_overshoot_bytes = max_overshoot_bytes.max(allocation.max_overshoot_bytes);
    }
    samples.sort_unstable();
    reset_counters();
    let baseline = LIVE_BYTES.with(Cell::get);
    black_box(op());
    let rust_allocation_calls = ALLOCATION_CALLS.with(Cell::get);
    let rust_peak_live_bytes = PEAK_BYTES.with(Cell::get).saturating_sub(baseline);
    object([
        ("case", string(name_for(language))),
        (
            "samples",
            integer(u64::try_from(SAMPLES).unwrap_or(u64::MAX)),
        ),
        ("median_ns", integer(samples[SAMPLES / 2])),
        ("p95_ns", integer(samples[SAMPLES * 95 / 100])),
        ("tree_allocation_calls", integer(tree_allocation_calls)),
        ("rust_allocation_calls", integer(rust_allocation_calls)),
        ("rust_peak_live_bytes", integer(rust_peak_live_bytes)),
        ("process_peak_rss_bytes", integer(process_peak_rss_bytes())),
        ("retained_bytes", integer(retained_bytes)),
        ("peak_bytes", integer(peak_bytes)),
        ("max_overshoot_bytes", integer(max_overshoot_bytes)),
    ])
}

const LANGUAGES: [Language; 5] = [
    Language::Python,
    Language::Rust,
    Language::JavaScript,
    Language::TypeScript,
    Language::Tsx,
];

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--list") => {
            for language in LANGUAGES {
                println!("{}", name_for(language));
            }
        }
        Some("--bench") | None => {
            let results: Vec<_> = LANGUAGES.into_iter().map(measure).collect();
            println!("{}", array(results).to_json());
        }
        Some(selected) => {
            let language = LANGUAGES
                .into_iter()
                .find(|language| name_for(*language) == selected)
                .unwrap_or_else(|| panic!("unknown syntax latency workload: {selected}"));
            println!("{}", measure(language).to_json());
        }
    }
}
