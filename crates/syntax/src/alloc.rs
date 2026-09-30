// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
#![allow(unsafe_code)]

//! Allocator wrappers routing tree-sitter's own C allocations.
//!
//! They go through [`std::alloc::System`] with per-parse accounting, so
//! `crates/syntax` can charge those allocations to a caller's
//! [`purrassist_core::Governor`] instead of leaving them ungoverned.
//!
//! # Scope of the `unsafe_code = "deny"` exception
//! The workspace denies `unsafe_code` everywhere else; this module is the one
//! exception (`#![allow(unsafe_code)]` above, scoped to this file only), and
//! it exists solely to implement [`tree_sitter::Allocator`]'s four
//! `unsafe extern "C" fn` pointers plus the raw header they share. See
//! `docs/DEPENDENCIES.md` and `SECURITY.md` for the registered rationale.
//!
//! # Process-global effect
//! [`tree_sitter::set_allocator`] replaces tree-sitter's C allocator for the
//! **entire process**, not just this crate's calls. `install` calls it
//! exactly once, guarded by a [`std::sync::Once`], before this crate ever
//! constructs a `Parser`, `Language`, `Query`, or `QueryCursor`. An embedder
//! that also links a *different* crate calling `tree_sitter::set_allocator`
//! (directly or via `ffi::ts_set_allocator`) is incompatible with this crate
//! in the same process: whichever call happens last wins, and tree-sitter's
//! own safety contract on `set_allocator` requires no live tree-sitter object
//! exist when either call is made. This crate never calls `set_allocator`
//! itself; every allocation it attributes to no active `Account` is still
//! tracked by the global [`live_bytes`]/[`peak_bytes`] counters, so an
//! embedder that skips this crate's `install` and lets tree-sitter keep its
//! default allocator is otherwise unaffected — this module's counters would
//! simply stay at zero.
//!
//! # Allocation failure is an abort, not a null return
//! Every call site inside tree-sitter's own C sources
//! (`array.h`, `subtree.c`, `stack.c`, `parser.c`, `query.c`, `tree.c`,
//! `wasm_store.c`) calls `ts_malloc`/`ts_calloc`/`ts_realloc` directly and
//! dereferences the result without checking for null — exactly like calling
//! the default allocator in `src/alloc.c`, which aborts the process on
//! allocation failure rather than returning null. `set_allocator`'s own
//! safety contract requires an installed allocator to preserve that
//! non-null-or-abort behavior. This module's wrappers honor it: a genuine
//! `std::alloc::System` allocation failure here aborts the process
//! (`std::process::abort`), the same failure mode tree-sitter's own default
//! allocator has. Budget enforcement never relies on returning null; it
//! happens exclusively at the progress-callback checkpoints in `lib.rs`,
//! which charge the *governor's* `ScratchBytes` budget and break the parse
//! with `BudgetExceeded`/`Cancelled` long before the process's real allocator
//! could be exhausted by a governed caller.
//!
//! # Per-parse accounting, never thread-local counters
//! Each allocated block's header stores a strong [`std::sync::Arc`] pointer
//! to the `Account` active when it was allocated (see `scoped`). Freeing
//! that block — from any thread, since tree-sitter's [`tree_sitter::Tree`] is
//! `Send` — reads the account straight out of the header and decrements its
//! shared, atomic counters directly; there is no thread-local counter to get
//! out of sync. The thread-local in this module (`CURRENT`) only *routes*
//! a fresh `malloc`/`calloc` to the right account; it is consulted at
//! allocation time, never at free time, and it holds no counts itself.
//!
//! # wasm32-wasip1
//! The WASI build links tree-sitter's C sources against a WASI C sysroot
//! (`vendor/wasi.json`), but tree-sitter's own allocation hook
//! (`ts_current_malloc`/`ts_current_calloc`/`ts_current_realloc`/
//! `ts_current_free`) is a C function pointer inside tree-sitter's own
//! compiled object, not the sysroot's `malloc`. `set_allocator` overwrites
//! those pointers the same way on every target `std::alloc::System` supports,
//! including `wasm32-wasip1`; `scripts/check-wasm.py` executes the identical
//! native/WASI acceptance and repair transcript and diffs their output
//! byte-for-byte, which exercises this module's allocator on that target the
//! same way `inspect`/`apply_checked`/`parse_governed` exercise it natively.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Once};

/// Alignment preserved for every block this module hands to tree-sitter.
///
/// `std::alloc::System` guarantees whatever alignment a `Layout` requests, so
/// this is not "the alignment libc's `malloc` happens to return" but a fixed,
/// generous bound (matching the common `max_align_t` on 64-bit platforms)
/// this module chooses and controls directly; every layout below explicitly
/// requests it.
const ALIGN: usize = 16;

/// One [`Header`] plus payload padding, rounded up to [`ALIGN`].
const HEADER_SIZE: usize = size_of::<Header>().div_ceil(ALIGN) * ALIGN;

/// The bookkeeping tree-sitter itself never sees, stored immediately before
/// every payload pointer this module returns.
#[repr(C)]
struct Header {
    /// Exact payload size passed to `malloc`/`calloc`, or the latest `size`
    /// passed to `realloc`.
    size: usize,
    /// Strong [`Arc`] pointer to the owning [`AccountInner`], or null when no
    /// `Account` was scoped at allocation time. Exactly one strong
    /// reference is attributed to each block; `realloc` carries it forward
    /// unchanged, and `free` releases it.
    account: *const AccountInner,
}

/// Process-wide byte counters covering every allocation this module's
/// wrappers ever see, attributed or not.
static GLOBAL_LIVE: AtomicU64 = AtomicU64::new(0);
static GLOBAL_PEAK: AtomicU64 = AtomicU64::new(0);

/// Live/peak counters shared by every clone of one `Account`.
#[derive(Debug)]
struct AccountInner {
    calls: AtomicU64,
    live: AtomicU64,
    peak: AtomicU64,
}

/// A per-parse (or per-tree, per-query) allocation accounting cell.
///
/// Cloning shares the same atomics; the crate creates exactly one `Account`
/// per governed parse and keeps a clone alive for as long as the operation
/// needs to observe it (see `lib.rs`'s `parse_accounted`). Dropping every
/// clone does not by itself free anything: outstanding allocation headers
/// still hold their own strong reference (see [`Header::account`]) until
/// each block is freed, so the counters stay valid until the last freed
/// block that was ever attributed to this account.
#[derive(Clone, Debug)]
pub(crate) struct Account(Arc<AccountInner>);

impl Account {
    /// A fresh account with zeroed counters.
    pub(crate) fn new() -> Self {
        Self(Arc::new(AccountInner {
            calls: AtomicU64::new(0),
            live: AtomicU64::new(0),
            peak: AtomicU64::new(0),
        }))
    }

    /// Successful malloc, calloc, and realloc calls attributed to this parse.
    pub(crate) fn calls(&self) -> u64 {
        self.0.calls.load(Ordering::Relaxed)
    }

    /// Bytes currently attributed to this account and not yet freed.
    pub(crate) fn live(&self) -> u64 {
        self.0.live.load(Ordering::Relaxed)
    }

    /// High-water mark of [`Account::live`] ever observed for this account.
    pub(crate) fn peak(&self) -> u64 {
        self.0.peak.load(Ordering::Relaxed)
    }
}

thread_local! {
    /// The account new `malloc`/`calloc` calls on this thread attribute
    /// their allocation to, set only by `scoped`. Consulted at allocation
    /// time only; frees never read it (see the module docs).
    static CURRENT: Cell<*const AccountInner> = const { Cell::new(std::ptr::null()) };
}

/// Restores the previously scoped account (if any) when `f` returns.
struct RestoreScope(*const AccountInner);
impl Drop for RestoreScope {
    fn drop(&mut self) {
        CURRENT.with(|cell| cell.set(self.0));
    }
}

/// Run `f`, attributing any tree-sitter allocation performed by *this
/// thread* during `f` to `account`. Restores the previously scoped account
/// (if any) when `f` returns, including if `f` panics.
pub(crate) fn scoped<R>(account: &Account, f: impl FnOnce() -> R) -> R {
    let previous = CURRENT.with(|cell| cell.replace(Arc::as_ptr(&account.0)));
    let _guard = RestoreScope(previous);
    f()
}

/// Currently live tree-sitter allocation, process-wide.
///
/// Covers every `Account` and any unattributed allocation. Only
/// tree-sitter's own C allocations are counted; ordinary Rust heap activity
/// elsewhere in the process is not observed here (see the module docs).
#[must_use]
pub fn live_bytes() -> u64 {
    GLOBAL_LIVE.load(Ordering::Relaxed)
}

/// High-water mark of [`live_bytes`].
///
/// Observed since process start or the last [`reset_peak_bytes`], whichever
/// is later.
#[must_use]
pub fn peak_bytes() -> u64 {
    GLOBAL_PEAK.load(Ordering::Relaxed)
}

/// Rebase [`peak_bytes`] to the current [`live_bytes`].
///
/// A subsequent [`peak_bytes`] read then reports only growth observed after
/// this call. Callers isolating one operation's measurement (qualification
/// tooling, tests) call this immediately before that operation; concurrent
/// tree-sitter activity on another thread can still be attributed to the
/// reset window, since these counters are process-wide, not per-call.
pub fn reset_peak_bytes() {
    let live = GLOBAL_LIVE.load(Ordering::Relaxed);
    GLOBAL_PEAK.store(live, Ordering::Relaxed);
}

static INSTALL: Once = Once::new();

/// Install this module's allocator wrappers as tree-sitter's process-wide
/// allocator, exactly once. Idempotent: later calls are no-ops. Every public
/// entry point in this crate that constructs a `Parser` calls this first,
/// before the `Parser` (or the `Language` it will use) exists, so tree-sitter
/// never allocates through its own default allocator on this crate's path.
pub(crate) fn install() {
    INSTALL.call_once(|| {
        // SAFETY: this is the first and only call to `set_allocator` this
        // process makes (guarded by `Once`, and this crate never calls it
        // again), made before this crate constructs any `Parser`/`Language`/
        // `Query`/`QueryCursor` (every such constructor is reached only
        // through `parse_accounted` in `lib.rs`, which calls `install`
        // first) — satisfying "call this before any other tree-sitter API
        // call, and do not call it again while live tree-sitter objects
        // exist". `ts_malloc`/`ts_calloc`/`ts_realloc`/`ts_free` below form
        // one allocator family (all route through `System` with the same
        // `Header` layout), never return null for a non-zero request
        // (aborting instead; see the module docs), and preserve `ALIGN`-byte
        // alignment, which is at least as strict as libc `malloc`'s
        // guarantee. This function is not thread-safe per
        // `set_allocator`'s own contract; `Once` provides the required
        // single-call serialization.
        unsafe {
            tree_sitter::set_allocator(Some(tree_sitter::Allocator {
                malloc: ts_malloc,
                calloc: ts_calloc,
                realloc: ts_realloc,
                free: ts_free,
            }));
        }
    });
}

/// Current thread-scoped account, or null when nothing is scoped.
fn current_account() -> *const AccountInner {
    CURRENT.with(Cell::get)
}

/// Build the fixed layout for one `HEADER_SIZE + payload` block.
fn layout_for(payload: usize) -> Layout {
    // A `payload` large enough to overflow `usize` alongside `HEADER_SIZE`,
    // or to violate `Layout`'s own size/alignment invariants, is not
    // something tree-sitter's own bounded structures ever request; treating
    // it as allocator exhaustion (abort, matching the module docs) rather
    // than silently truncating is the only behavior compatible with never
    // returning null for a request tree-sitter believes it made.
    HEADER_SIZE
        .checked_add(payload)
        .and_then(|size| Layout::from_size_align(size, ALIGN).ok())
        .unwrap_or_else(|| std::process::abort())
}

/// Attribute `amount` newly allocated bytes to `account` (if any) and to the
/// global counters, updating both peaks.
fn account_track(account: *const AccountInner, amount: u64) {
    let live = GLOBAL_LIVE.fetch_add(amount, Ordering::Relaxed) + amount;
    GLOBAL_PEAK.fetch_max(live, Ordering::Relaxed);
    if !account.is_null() {
        // SAFETY: `account` is a live `AccountInner`: it was produced by
        // `Arc::as_ptr` on an `Account` held on some caller's stack for the
        // duration of a `scoped` call (see `current_account`/`scoped`), and
        // this block now holds its own strong reference (see
        // `attribute_new_block`), keeping the allocation valid regardless of
        // whether that original `Account` has since been dropped.
        let inner = unsafe { &*account };
        inner.calls.fetch_add(1, Ordering::Relaxed);
        let live = inner.live.fetch_add(amount, Ordering::Relaxed) + amount;
        inner.peak.fetch_max(live, Ordering::Relaxed);
    }
}

/// Remove `amount` bytes previously tracked for `account` (if any) and for
/// the global counters. Does not touch the block's strong reference; callers
/// that are freeing (not resizing) the block release that separately.
fn account_untrack(account: *const AccountInner, amount: u64) {
    GLOBAL_LIVE.fetch_sub(amount, Ordering::Relaxed);
    if !account.is_null() {
        // SAFETY: as in `account_track`.
        let inner = unsafe { &*account };
        inner.live.fetch_sub(amount, Ordering::Relaxed);
    }
}

/// Stamp a freshly allocated block's header and take out the one strong
/// reference on the currently scoped account that this block will carry
/// until it is freed (see [`Header::account`]).
fn attribute_new_block(header: *mut Header, payload: usize) {
    let current = current_account();
    let account = if current.is_null() {
        std::ptr::null()
    } else {
        // SAFETY: `current` was produced by `Arc::as_ptr` on an `Account`
        // still held by the `scoped` caller (see `current_account`); taking
        // out one more strong reference here is exactly
        // `Arc::increment_strong_count`'s documented FFI use: attributing an
        // additional owned reference to a raw pointer whose `Arc` is
        // currently live.
        unsafe { Arc::increment_strong_count(current) };
        current
    };
    // SAFETY: `header` points to a fresh, writable, `Header`-aligned
    // location (the caller just allocated at least `HEADER_SIZE` bytes at
    // `ALIGN` for it).
    unsafe {
        header.write(Header {
            size: payload,
            account,
        });
    }
    account_track(account, u64::try_from(payload).unwrap_or(u64::MAX));
}

/// Allocate one `HEADER_SIZE + payload` block, stamp its header, and return
/// the payload pointer. Aborts on genuine allocator exhaustion (see the
/// module docs); never returns null otherwise.
///
/// # Safety
/// None beyond `payload` being a size a `Layout` can represent alongside
/// `HEADER_SIZE`, which [`layout_for`] itself guards.
unsafe fn allocate(payload: usize) -> *mut c_void {
    let layout = layout_for(payload);
    // SAFETY: `layout` has a non-zero size (at least `HEADER_SIZE`) and a
    // valid power-of-two alignment (`ALIGN`), satisfying `GlobalAlloc::alloc`.
    let base = unsafe { System.alloc(layout) };
    if base.is_null() {
        // See the module docs: no correct non-null pointer exists to return.
        std::process::abort();
    }
    // `base` is `ALIGN`-aligned by `layout`, which is stricter than
    // `Header`'s own (8-byte) alignment requirement, so this cast is sound
    // despite the pointee being more strictly aligned than `u8`.
    #[allow(
        clippy::cast_ptr_alignment,
        reason = "base is ALIGN(16)-aligned, exceeding Header's 8-byte requirement"
    )]
    attribute_new_block(base.cast::<Header>(), payload);
    // SAFETY: `base` is a `System`-allocated block of at least
    // `HEADER_SIZE + payload` bytes at `ALIGN`; `HEADER_SIZE` is in bounds
    // and itself a multiple of `ALIGN`, so the result stays `ALIGN`-aligned.
    unsafe { base.add(HEADER_SIZE).cast::<c_void>() }
}

/// Read back the header immediately preceding a payload pointer this module
/// returned.
///
/// # Safety
/// `pointer` must be a non-null payload pointer previously returned by
/// [`allocate`] or [`reallocate`] and not yet freed.
const unsafe fn header_of(pointer: *mut c_void) -> (*mut u8, Header) {
    // SAFETY: `pointer` is `HEADER_SIZE` bytes past the block's start (see
    // `allocate`/`reallocate`), so subtracting `HEADER_SIZE` stays in bounds
    // of the same allocation.
    let base = unsafe { pointer.cast::<u8>().sub(HEADER_SIZE) };
    // SAFETY: `base` points to a valid, initialized `Header` written by
    // `allocate`/`reallocate` for this exact block; reading it by value
    // copies the (`Copy`-eligible) fields out without invalidating the
    // header still stored there. `base` is `ALIGN`(16)-aligned, exceeding
    // `Header`'s 8-byte alignment requirement, so the cast below is sound.
    #[allow(clippy::cast_ptr_alignment)]
    let header = unsafe { base.cast::<Header>().read() };
    (base, header)
}

/// # Safety
/// Called only by tree-sitter, with a `size` it computed for its own
/// bounded internal structures.
unsafe extern "C" fn ts_malloc(size: usize) -> *mut c_void {
    // SAFETY: delegates to `allocate`, whose only precondition `layout_for`
    // itself enforces.
    unsafe { allocate(size) }
}

/// # Safety
/// Called only by tree-sitter, with `count`/`size` it computed for its own
/// bounded internal structures.
unsafe extern "C" fn ts_calloc(count: usize, size: usize) -> *mut c_void {
    let Some(total) = count.checked_mul(size) else {
        // A `count * size` overflow is not something tree-sitter's own
        // bounded structures produce; there is no correct non-null pointer
        // to return for it (see the module docs), so this aborts exactly as
        // genuine allocator exhaustion would.
        std::process::abort();
    };
    // SAFETY: delegates to `allocate` for the sizing/attribution, then
    // zeroes exactly the `total` payload bytes it just allocated, matching
    // `calloc`'s contract; `pointer` is non-null (checked above `allocate`
    // itself aborts rather than returning null).
    unsafe {
        let pointer = allocate(total);
        std::ptr::write_bytes(pointer.cast::<u8>(), 0, total);
        pointer
    }
}

/// # Safety
/// Called only by tree-sitter with either a null pointer (equivalent to
/// `malloc`) or a non-null pointer this module previously returned and has
/// not yet freed.
unsafe extern "C" fn ts_realloc(pointer: *mut c_void, size: usize) -> *mut c_void {
    if pointer.is_null() {
        // SAFETY: matches libc `realloc(NULL, size)`, equivalent to `malloc`.
        return unsafe { allocate(size) };
    }
    // SAFETY: `pointer` was returned by `ts_malloc`/`ts_calloc`/`ts_realloc`
    // and not yet freed (tree-sitter never passes a foreign pointer to
    // `ts_realloc`), satisfying `reallocate`'s precondition.
    unsafe { reallocate(pointer, size) }
}

/// Resize an existing block in place (from `System`'s perspective), carrying
/// its attribution forward unchanged and updating the counters by the
/// old/new size delta.
///
/// # Safety
/// `pointer` must be a non-null payload pointer previously returned by
/// [`allocate`] or [`reallocate`] and not yet freed.
unsafe fn reallocate(pointer: *mut c_void, new_size: usize) -> *mut c_void {
    // SAFETY: precondition forwarded from the caller.
    let (base, header) = unsafe { header_of(pointer) };
    let old_layout = layout_for(header.size);
    let new_layout = layout_for(new_size);
    // SAFETY: `base`/`old_layout` describe exactly the block `System`
    // returned for this pointer (`layout_for` is a pure function of
    // `header.size`, which matches what was charged when this block was
    // last allocated or reallocated); `new_layout.size() > 0`.
    let new_base = unsafe { System.realloc(base, old_layout, new_layout.size()) };
    if new_base.is_null() {
        // See `allocate`: no correct non-null pointer to return. The
        // original block is untouched on `realloc` failure, so this is a
        // deliberate abort, not a use-after-free.
        std::process::abort();
    }
    account_untrack(
        header.account,
        u64::try_from(header.size).unwrap_or(u64::MAX),
    );
    // SAFETY: `new_base` is a fresh `System`-allocated block of at least
    // `HEADER_SIZE + new_size` bytes at `ALIGN`; writing the header carries
    // the same `account` pointer forward without touching its strong count
    // (this block still owns exactly the one reference `allocate` took out
    // for it, or that a prior `reallocate` carried forward). `new_base` is
    // `ALIGN`(16)-aligned, exceeding `Header`'s 8-byte alignment requirement,
    // so the cast below is sound.
    #[allow(clippy::cast_ptr_alignment)]
    unsafe {
        new_base.cast::<Header>().write(Header {
            size: new_size,
            account: header.account,
        });
    }
    account_track(header.account, u64::try_from(new_size).unwrap_or(u64::MAX));
    // SAFETY: as in `allocate`.
    unsafe { new_base.add(HEADER_SIZE).cast::<c_void>() }
}

/// # Safety
/// Called only by tree-sitter with either a null pointer (a no-op, matching
/// libc `free`) or a non-null pointer this module previously returned and
/// has not yet freed.
unsafe extern "C" fn ts_free(pointer: *mut c_void) {
    if pointer.is_null() {
        return;
    }
    // SAFETY: precondition forwarded from the caller.
    let (base, header) = unsafe { header_of(pointer) };
    account_untrack(
        header.account,
        u64::try_from(header.size).unwrap_or(u64::MAX),
    );
    if !header.account.is_null() {
        // SAFETY: `header.account` holds exactly one strong-count unit
        // attributed to this block by `attribute_new_block`'s
        // `Arc::increment_strong_count` call (carried forward unchanged by
        // any intervening `reallocate`). `Arc::from_raw`'s documented
        // contract accepts reclaiming a unit created that way, not only one
        // created by `Arc::into_raw`; reconstructing and immediately
        // dropping an owned `Arc` here releases exactly that one unit.
        // `AccountInner` is deallocated only once every block ever
        // attributed to it (and the original `Account` handle) has
        // released its reference.
        unsafe { drop(Arc::from_raw(header.account)) };
    }
    let layout = layout_for(header.size);
    // SAFETY: `base` was allocated by `System.alloc`/`System.realloc` with
    // exactly this layout (same size derivation via `layout_for` and the
    // same fixed `ALIGN`).
    unsafe { System.dealloc(base, layout) };
}

#[cfg(test)]
mod tests {
    use super::{Account, install, live_bytes, peak_bytes, reset_peak_bytes, scoped};
    use std::sync::Mutex;

    /// [`live_bytes`]/[`peak_bytes`] are process-wide, and `cargo test` runs
    /// this file's tests as concurrent threads within one process; each test
    /// below holds this lock for its *entire* body so no other test's
    /// tree-sitter activity can move these counters mid-assertion.
    static LOCK: Mutex<()> = Mutex::new(());

    /// `install` takes effect before the very first `Parser`/`Language` use
    /// on this path: this crate's shared parsing helper calls `install` as
    /// its first statement, strictly before constructing a `Parser`, on
    /// every call. If that ordering were wrong (allocator installed too
    /// late, or not at all), tree-sitter would keep using its own default
    /// allocator and none of these counters would ever move.
    #[test]
    fn install_precedes_any_tree_sitter_allocation() {
        let _guard = LOCK.lock().unwrap();
        install();
        reset_peak_bytes();
        let account = Account::new();
        let tree = scoped(&account, || {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&tree_sitter_rust::LANGUAGE.into())
                .unwrap();
            parser.parse("fn f() {}", None)
        });
        assert!(tree.is_some());
        assert!(
            peak_bytes() > 0,
            "installing the allocator before parsing must make tree-sitter's \
             own allocations visible to this module's counters"
        );
        drop(tree);
        assert_eq!(live_bytes(), 0, "the parser and tree dropped above");
    }

    #[test]
    fn account_tracks_only_allocations_made_while_scoped() {
        let _guard = LOCK.lock().unwrap();
        install();
        let account = Account::new();
        assert_eq!(account.live(), 0);
        let tree = scoped(&account, || {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&tree_sitter_rust::LANGUAGE.into())
                .unwrap();
            parser.parse("fn f() { 1 + 1 }", None)
        });
        assert!(account.live() > 0);
        assert!(account.calls() > 0);
        assert!(account.peak() >= account.live());
        drop(tree);
        assert_eq!(account.live(), 0);
    }

    #[test]
    fn call_counts_include_resizes_and_zeroed_allocations_but_not_frees() {
        let _guard = LOCK.lock().unwrap();
        let account = Account::new();
        scoped(&account, || {
            // SAFETY: these calls use valid sizes; realloc and free receive
            // exactly the live pointers produced by this allocator, once each.
            unsafe {
                let first = super::ts_malloc(8);
                assert_eq!((account.calls(), account.live()), (1, 8));
                let first = super::ts_realloc(first, 16);
                assert_eq!((account.calls(), account.live()), (2, 16));
                let second = super::ts_calloc(4, 8);
                assert_eq!((account.calls(), account.live()), (3, 48));
                super::ts_free(first);
                super::ts_free(second);
            }
        });
        assert_eq!(
            (account.calls(), account.live(), account.peak()),
            (3, 0, 48)
        );
    }

    #[test]
    fn dropping_a_tree_on_another_thread_returns_its_account_to_zero() {
        let _guard = LOCK.lock().unwrap();
        install();
        let account = Account::new();
        let tree = scoped(&account, || {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&tree_sitter_rust::LANGUAGE.into())
                .unwrap();
            parser.parse("fn f() { 1 + 1 }", None)
        })
        .unwrap();
        assert!(account.live() > 0);
        std::thread::spawn(move || drop(tree)).join().unwrap();
        assert_eq!(account.live(), 0);
    }
}
