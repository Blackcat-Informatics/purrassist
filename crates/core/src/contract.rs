// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Shared assistance, resource-accounting, and evidence contracts.

use crate::digest::Digest;
use core::fmt;
use core::mem;
use core::ops::Range;
use std::collections::TryReserveError;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

fn size(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// Caller-owned, monotonic cancellation signal shared by related operations.
///
/// Clones share one signal. Cancelling never undoes completed effects; governed
/// operations observe it at their next check or charge. No background task,
/// clock, or ambient process state is consulted. Create a new token for a retry.
///
/// ```
/// use purrassist_core::{Budget, CancellationToken, Governor, StopReason};
/// let token = CancellationToken::new();
/// let governor = Governor::with_cancellation(Budget::default(), token.clone());
/// token.cancel();
/// assert_eq!(governor.check(), Err(StopReason::Cancelled));
/// ```
#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    /// Create an uncancelled signal. Allocation belongs to the caller.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Irreversibly request cancellation for every clone and attached governor.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether cancellation has been requested. This is not an effect rollback.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Version of the machine-readable receipt contract.
///
/// # History
/// - `1`: initial wire shape (see `docs/schema/receipt.schema.json` history
///   for the exact JSON Schema of each version, and
///   `crates/core/tests/data/receipts/` for one golden fixture per version).
/// - `2`: `usage` gained `total_scratch_charged` (the monotone cumulative
///   scratch counter, distinct from the existing high-water
///   `peak_scratch_bytes`; see [`Usage::total_scratch_charged`]).
///   `AlternativeReceipt.evidence`/`.losses`/`.inferences` now hold only what
///   that alternative alone depends on: evidence, losses, and inferences
///   shared by *every* competing alternative are promoted to the top-level
///   `evidence`/`losses`/`inferences` ledgers instead of being duplicated
///   into each alternative (see `crate::provenance`). Callers that assumed
///   an alternative's ledgers were self-contained must also read the
///   top-level ledgers to see shared transformations.
pub const RECEIPT_VERSION: u32 = 2;

/// Increasing recovery effort; all presets remain finite.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Effort {
    /// Validate exact input, without generating repairs.
    Exact,
    /// Resolve simple local mistakes.
    Quick,
    /// Search composed repairs with ordinary resource bounds.
    #[default]
    Standard,
    /// Explore a wider set of competing interpretations.
    Deep,
    /// Use the largest standard finite search budget.
    Maximum,
}

impl Effort {
    /// Reproducible default limits; callers may provide explicit limits instead.
    #[must_use]
    pub const fn budget(self) -> Budget {
        let (work, candidates) = match self {
            Self::Exact => (100_000, 1),
            Self::Quick => (250_000, 16),
            Self::Standard => (1_000_000, 128),
            Self::Deep => (10_000_000, 1024),
            Self::Maximum => (50_000_000, 8192),
        };
        Budget {
            work,
            candidates,
            input_bytes: 16 * 1024 * 1024,
            scratch_bytes: 64 * 1024 * 1024,
            depth: 128,
            evidence_entries: 65_536,
            helper_calls: 0,
        }
    }
}

/// Explicit limits. Helper access is disabled until the caller budgets it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    /// Maximum algorithmic work units.
    pub work: u64,
    /// Maximum candidate proposals.
    pub candidates: u64,
    /// Maximum bytes admitted as one source.
    pub input_bytes: u64,
    /// Maximum simultaneously reserved scratch bytes.
    pub scratch_bytes: u64,
    /// Maximum structural nesting depth.
    pub depth: u32,
    /// Maximum evidence entries.
    pub evidence_entries: u64,
    /// Maximum externally fulfilled helper requests.
    pub helper_calls: u64,
}

impl Default for Budget {
    fn default() -> Self {
        Effort::Standard.budget()
    }
}

/// A charged resource dimension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    /// Algorithmic work.
    Work,
    /// Proposed candidates.
    Candidates,
    /// Admitted source bytes.
    InputBytes,
    /// Live scratch storage.
    ScratchBytes,
    /// Structural recursion depth.
    Depth,
    /// Audit evidence entries.
    EvidenceEntries,
    /// External helper invocations.
    HelperCalls,
}

/// Why an operation stopped; exhaustion never asserts unsatisfiability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// The requested operation completed.
    Completed,
    /// The finite candidate space examined produced no acceptable result.
    NoCandidate,
    /// Continuing would exceed the stated resource limit.
    BudgetExceeded(Resource),
    /// The caller cancelled the operation.
    Cancelled,
    /// Input or configuration cannot be interpreted under its contract.
    InvalidInput(String),
}

impl fmt::Display for StopReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Completed => formatter.write_str("completed"),
            Self::NoCandidate => formatter.write_str("no acceptable candidate found"),
            Self::BudgetExceeded(resource) => write!(formatter, "budget exceeded: {resource:?}"),
            Self::Cancelled => formatter.write_str("cancelled"),
            Self::InvalidInput(detail) => write!(formatter, "invalid input: {detail}"),
        }
    }
}

impl std::error::Error for StopReason {}

/// Measured consumption, distinct from configured bounds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Charged work units.
    pub work: u64,
    /// Candidate proposals charged.
    pub candidates: u64,
    /// High-water scratch reservation: the largest amount ever *simultaneously*
    /// live. A charge/release loop keeps this low even when it performs many
    /// real allocations in sequence; see `total_scratch_charged` for that case.
    pub peak_scratch_bytes: u64,
    /// Every scratch byte ever charged, summed, never reduced by a release.
    /// Unlike `peak_scratch_bytes`, this bounds *cumulative* allocation over
    /// an operation with a charge/release loop (fuzzy edit-distance scans,
    /// regex backtracking, per-candidate ranking), where the allocator
    /// performs one real call per iteration even though live scratch returns
    /// to a low water mark between iterations.
    pub total_scratch_charged: u64,
    /// Evidence entries recorded.
    pub evidence_entries: u64,
    /// Helper requests charged.
    pub helper_calls: u64,
}

impl Usage {
    /// Combine two independently measured `Usage` values into the usage of
    /// their combined operation: `work`, `candidates`, `evidence_entries`,
    /// `helper_calls`, and `total_scratch_charged` (all monotone counters)
    /// sum, while `peak_scratch_bytes` (a high-water mark, not a counter)
    /// takes the larger of the two, since the two measurements' live scratch
    /// was never necessarily simultaneous.
    ///
    /// Use this instead of merging individual fields by hand when combining
    /// the usage of two governors that covered sequential phases of the same
    /// logical operation (for example, a helper request's preparation and
    /// its completion; see `purrassist_host::helpers`).
    ///
    /// ```
    /// use purrassist_core::contract::Usage;
    /// let first = Usage { work: 10, peak_scratch_bytes: 100, total_scratch_charged: 100, ..Usage::default() };
    /// let second = Usage { work: 5, peak_scratch_bytes: 40, total_scratch_charged: 40, ..Usage::default() };
    /// let merged = first.merge(&second);
    /// assert_eq!(merged.work, 15);
    /// assert_eq!(merged.peak_scratch_bytes, 100);
    /// assert_eq!(merged.total_scratch_charged, 140);
    /// ```
    #[must_use]
    pub fn merge(&self, other: &Self) -> Self {
        Self {
            work: self.work.saturating_add(other.work),
            candidates: self.candidates.saturating_add(other.candidates),
            peak_scratch_bytes: self.peak_scratch_bytes.max(other.peak_scratch_bytes),
            total_scratch_charged: self
                .total_scratch_charged
                .saturating_add(other.total_scratch_charged),
            evidence_entries: self.evidence_entries.saturating_add(other.evidence_entries),
            helper_calls: self.helper_calls.saturating_add(other.helper_calls),
        }
    }
}

#[derive(Debug)]
struct ScratchMeterInner {
    limit: u64,
    live: AtomicU64,
    peak: AtomicU64,
    /// Monotone sum of every successful charge; never decremented by
    /// `release`. See [`Usage::total_scratch_charged`].
    total_charged: AtomicU64,
    /// Sticky once set: a scratch dimension that has ever exceeded its limit
    /// keeps failing even if bytes are later released, matching the
    /// governor's overall sticky-exhaustion contract.
    exhausted: AtomicBool,
}

/// The shared, atomic scratch-byte counter behind a [`Governor`] and every
/// [`ScratchLease`]/[`ScratchVec`]/[`ScratchString`]/[`ScratchMap`] it issues.
///
/// Cloning shares the same live/peak counters and the same limit, the same
/// pattern as [`CancellationToken`]: a scratch-aware collection created from a
/// governor keeps charging that governor's budget after the call that created
/// it returns, and its `Drop` releases exactly what it charged. A [`Governor`]
/// clone does not share its meter (see [`Governor::clone`](struct.Governor.html));
/// only handles issued *from one governor* to its own collections do.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// let mut governor = Governor::new(Budget {
///     scratch_bytes: 8,
///     ..Budget::default()
/// });
/// let meter = governor.scratch_meter();
/// assert_eq!(meter.charge(8), Ok(()));
/// assert_eq!(meter.peak(), 8);
/// assert!(meter.charge(1).is_err()); // over the limit: `meter` is now exhausted.
/// assert!(meter.is_exhausted());
/// assert_eq!(meter.release(8), Ok(())); // releasing does not clear stickiness.
/// assert!(meter.charge(1).is_err());
/// ```
#[derive(Clone, Debug)]
pub struct ScratchMeter(Arc<ScratchMeterInner>);

impl ScratchMeter {
    fn new(limit: u64) -> Self {
        Self(Arc::new(ScratchMeterInner {
            limit,
            live: AtomicU64::new(0),
            peak: AtomicU64::new(0),
            total_charged: AtomicU64::new(0),
            exhausted: AtomicBool::new(false),
        }))
    }

    /// An independent meter with the same limit and the same live/peak/total
    /// snapshot, sharing no state with `self`. Used by [`Governor::clone`] so
    /// clones keep independent accounting.
    fn snapshot(&self) -> Self {
        let copy = Self::new(self.0.limit);
        copy.0
            .live
            .store(self.0.live.load(Ordering::Relaxed), Ordering::Relaxed);
        copy.0
            .peak
            .store(self.0.peak.load(Ordering::Relaxed), Ordering::Relaxed);
        copy.0.total_charged.store(
            self.0.total_charged.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        copy.0
            .exhausted
            .store(self.0.exhausted.load(Ordering::Relaxed), Ordering::Relaxed);
        copy
    }

    /// Charge `amount` bytes, failing before any allocation if this would
    /// exceed the limit, or if this dimension is already sticky-exhausted.
    ///
    /// # Errors
    /// Returns `BudgetExceeded(ScratchBytes)` without changing the live amount.
    pub fn charge(&self, amount: u64) -> Result<(), StopReason> {
        if self.0.exhausted.load(Ordering::Relaxed) {
            return Err(StopReason::BudgetExceeded(Resource::ScratchBytes));
        }
        let mut current = self.0.live.load(Ordering::Relaxed);
        loop {
            let Some(next) = current
                .checked_add(amount)
                .filter(|next| *next <= self.0.limit)
            else {
                self.0.exhausted.store(true, Ordering::Relaxed);
                return Err(StopReason::BudgetExceeded(Resource::ScratchBytes));
            };
            match self.0.live.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.0.peak.fetch_max(next, Ordering::Relaxed);
                    self.0.total_charged.fetch_add(amount, Ordering::Relaxed);
                    return Ok(());
                }
                Err(actual) => current = actual,
            }
        }
    }

    /// Release a previous charge.
    ///
    /// # Errors
    /// Rejects an unbalanced release (more bytes than are currently live),
    /// leaving the live amount unchanged.
    pub fn release(&self, amount: u64) -> Result<(), StopReason> {
        let mut current = self.0.live.load(Ordering::Relaxed);
        loop {
            let Some(next) = current.checked_sub(amount) else {
                return Err(StopReason::InvalidInput(
                    "unbalanced scratch reservation".into(),
                ));
            };
            match self.0.live.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(()),
                Err(actual) => current = actual,
            }
        }
    }

    /// Currently live (charged, not yet released) scratch bytes.
    #[must_use]
    pub fn live(&self) -> u64 {
        self.0.live.load(Ordering::Relaxed)
    }

    /// High-water live scratch bytes ever observed.
    #[must_use]
    pub fn peak(&self) -> u64 {
        self.0.peak.load(Ordering::Relaxed)
    }

    /// Every scratch byte ever successfully charged, summed; never reduced
    /// by [`ScratchMeter::release`]. See [`Usage::total_scratch_charged`].
    #[must_use]
    pub fn total_charged(&self) -> u64 {
        self.0.total_charged.load(Ordering::Relaxed)
    }

    /// Whether this dimension has ever exceeded its limit.
    #[must_use]
    pub fn is_exhausted(&self) -> bool {
        self.0.exhausted.load(Ordering::Relaxed)
    }
}

/// An opaque byte reservation, charged eagerly and released exactly once,
/// automatically, when dropped.
///
/// Use [`Governor::lease_scratch`] for a transient allocation whose exact
/// layout does not need a typed collection: for example, a clone made while
/// composing a candidate that may or may not be kept. The lease holds only a
/// [`ScratchMeter`] clone, not a governor borrow, so it can be returned from a
/// function and dropped (or [`retain`](ScratchLease::retain)ed) by the caller.
///
/// ```
/// use purrassist_core::{Budget, Governor, contract::Resource};
/// let mut governor = Governor::new(Budget::default());
/// {
///     let _lease = governor.lease_scratch(64).expect("within budget");
///     assert_eq!(governor.usage().peak_scratch_bytes, 64);
/// } // released here; a later charge sees the freed bytes again.
/// assert_eq!(governor.charge(Resource::ScratchBytes, 64), Ok(()));
/// ```
#[derive(Debug)]
pub struct ScratchLease {
    meter: ScratchMeter,
    bytes: u64,
}

impl ScratchLease {
    const fn new(meter: ScratchMeter, bytes: u64) -> Self {
        Self { meter, bytes }
    }

    /// Bytes currently held by this lease.
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Keep the charge permanently instead of releasing it on drop, because
    /// the reservation turned out to back retained output rather than
    /// transient scratch (mirrors [`ScratchVec::into_vec`]).
    pub const fn retain(self) {
        mem::forget(self);
    }
}

impl Drop for ScratchLease {
    fn drop(&mut self) {
        let _ = self.meter.release(self.bytes);
    }
}

/// A type's portable, target-independent scratch charge.
///
/// [`ScratchVec`] and [`ScratchMap`] must charge the same number of bytes for
/// the same workload on every compilation target so a governor makes the
/// same admit/reject decision, and a receipt reports the same
/// `peak_scratch_bytes`/`total_scratch_charged`, on 32-bit WASM as on 64-bit
/// native (see `docs/QUALIFICATION.md`). `core::mem::size_of::<T>()` is the
/// wrong tool for that: it varies with pointer width for any `T` containing
/// a `String`, `Vec`, `Box`, or reference, which is exactly the shape of the
/// element types these containers hold.
///
/// [`ScratchCharge::CHARGE`] is instead a literal byte count fixed at its
/// *64-bit* layout size, hand-measured once and never recomputed from
/// `size_of`. Every implementation is paired with a compile-time assertion
/// (`scratch_charge!` generates it; the blanket tuple impl below inlines it)
/// that `size_of::<T>() <= T::CHARGE` on whichever target is actually being
/// compiled, so the portable charge can only ever *over*-count a target
/// narrower than 64 bits, never under-count real memory.
pub trait ScratchCharge {
    /// This type's charge, in bytes, under the 64-bit layout.
    const CHARGE: usize;
}

/// Implement [`ScratchCharge`] for a concrete (non-generic) type with a
/// literal charge, and assert that charge never undercounts `size_of::<T>()`
/// on the target actually compiling this crate.
macro_rules! scratch_charge {
    ($ty:ty, $charge:expr) => {
        impl ScratchCharge for $ty {
            const CHARGE: usize = $charge;
        }
        const _: () = assert!(
            core::mem::size_of::<$ty>() <= <$ty as ScratchCharge>::CHARGE,
            concat!(
                "ScratchCharge for ",
                stringify!($ty),
                " undercounts size_of on this target"
            )
        );
    };
}

// Primitive element types actually stored in a `ScratchVec`/`ScratchMap`
// (directly, or as a tuple component) anywhere in this crate's production
// code, its tests, or its doctests. Each charge is that type's `size_of` on
// a 64-bit target, so these constants never change even when this crate is
// compiled for wasm32.
scratch_charge!((), 0);
scratch_charge!(bool, 1);
scratch_charge!(u32, 4);
scratch_charge!(u64, 8);
scratch_charge!(usize, 8);
scratch_charge!(&'static str, 16); // a `&str` is a two-word (ptr, len) fat pointer.
scratch_charge!(String, 24); // `Vec<u8>`'s (ptr, len, cap) layout.
scratch_charge!(crate::json::Node, 56);
scratch_charge!(crate::json::Member, 104);
scratch_charge!(crate::provenance::NodeId, 4);
scratch_charge!(crate::pattern::derive::Term, 32); // largest variant: `Repeat(u32, u64, u64, bool)`.
scratch_charge!(crate::provenance::ProvenanceNode, 160);

/// Round `bytes` up to the next 8-byte word, the same rounding an
/// 8-byte-aligned struct field's own size implicitly gets.
const fn round_up_to_word(bytes: usize) -> usize {
    (bytes + 7) & !7
}

/// Every field of a tuple, struct, or enum used as one `ScratchVec`/
/// `ScratchMap` element charges as if each of its fields were individually
/// padded out to an 8-byte word, which only ever over-counts a real,
/// field-reordered layout's size (Rust already minimizes padding by
/// reordering fields; summing each field's *own* rounded size can only add
/// more slack than that reordering leaves). `(K, V)` is the only such
/// composite [`ScratchMap`] needs charged this way, since a `ScratchMap` is
/// backed by `Vec<(K, V)>` (see [`ScratchMap`]'s doc comment).
impl<A: ScratchCharge, B: ScratchCharge> ScratchCharge for (A, B) {
    const CHARGE: usize = round_up_to_word(A::CHARGE) + round_up_to_word(B::CHARGE);
}
const _: () = {
    // Every concrete tuple instantiation actually used as a `ScratchMap`
    // element in this crate, checked eagerly here rather than only at its
    // (possibly untested) call site.
    assert!(core::mem::size_of::<(String, ())>() <= <(String, ()) as ScratchCharge>::CHARGE);
    assert!(
        core::mem::size_of::<(String, crate::provenance::NodeId)>()
            <= <(String, crate::provenance::NodeId) as ScratchCharge>::CHARGE
    );
    assert!(
        core::mem::size_of::<(u32, &'static str)>()
            <= <(u32, &'static str) as ScratchCharge>::CHARGE
    );
};

/// Growth strategy shared by [`ScratchVec`], [`ScratchString`], and
/// [`ScratchMap`]: charge for doubled capacity before reserving it, so a
/// sequence of pushes amortizes to O(1) charges like `Vec`'s own growth.
fn grown_capacity(len: usize, capacity: usize, additional: usize) -> Option<usize> {
    let needed = len.checked_add(additional)?;
    if needed <= capacity {
        return Some(capacity);
    }
    Some(needed.max(capacity.saturating_mul(2)).max(4))
}

fn reserve_failure(error: &TryReserveError) -> StopReason {
    StopReason::InvalidInput(format!("scratch allocation failed: {error}"))
}

/// A `Vec<T>` whose growth is charged against a [`ScratchMeter`] before it is
/// requested from the allocator, and whose charge is released exactly once,
/// on drop.
///
/// Growth charges `capacity_delta * T::CHARGE` bytes (see [`ScratchCharge`],
/// *not* `size_of::<T>()`, so the charge is the same on every target), then
/// calls [`Vec::try_reserve_exact`]; on the rare allocator failure the
/// just-charged bytes are released and `BudgetExceeded(ScratchBytes)` is
/// returned — this type never aborts on allocation failure. Charging only
/// covers the vector's own backing buffer: an element that owns further heap
/// memory (a `String` inside a `ScratchVec<String>`, for example) must be
/// charged separately before it is constructed.
///
/// ```
/// use purrassist_core::{Budget, Governor, ScratchVec};
/// let mut governor = Governor::new(Budget::default());
/// let mut values: ScratchVec<u32> = ScratchVec::new(&governor);
/// values.push(1).unwrap();
/// values.push(2).unwrap();
/// assert_eq!(values.as_slice(), &[1, 2]);
/// assert!(governor.usage().peak_scratch_bytes > 0);
/// let plain: Vec<u32> = values.into_vec(); // charge is now permanently retained
/// assert_eq!(plain, vec![1, 2]);
/// ```
#[derive(Debug)]
pub struct ScratchVec<T> {
    inner: Vec<T>,
    meter: ScratchMeter,
    reserved: u64,
}

impl<T: ScratchCharge> ScratchVec<T> {
    /// An empty vector charging against `governor`'s scratch meter.
    #[must_use]
    pub fn new(governor: &Governor) -> Self {
        Self {
            inner: Vec::new(),
            meter: governor.scratch_meter(),
            reserved: 0,
        }
    }

    /// Ensure capacity for `additional` more elements, charging before
    /// reserving.
    ///
    /// # Errors
    /// Returns `BudgetExceeded(ScratchBytes)` before any allocation.
    pub fn reserve(&mut self, additional: usize) -> Result<(), StopReason> {
        let capacity = self.inner.capacity();
        let Some(target) = grown_capacity(self.inner.len(), capacity, additional) else {
            return Err(StopReason::BudgetExceeded(Resource::ScratchBytes));
        };
        if target == capacity {
            return Ok(());
        }
        let delta = size(target - capacity).saturating_mul(size(T::CHARGE.max(1)));
        self.meter.charge(delta)?;
        self.reserved = self.reserved.saturating_add(delta);
        if let Err(error) = self.inner.try_reserve_exact(target - self.inner.len()) {
            let _ = self.meter.release(delta);
            self.reserved = self.reserved.saturating_sub(delta);
            return Err(reserve_failure(&error));
        }
        Ok(())
    }

    /// Push one element, growing (and charging) first if needed.
    ///
    /// # Errors
    /// Returns `BudgetExceeded(ScratchBytes)` before any allocation.
    pub fn push(&mut self, value: T) -> Result<(), StopReason> {
        if self.inner.len() == self.inner.capacity() {
            self.reserve(1)?;
        }
        self.inner.push(value);
        Ok(())
    }

    /// Push every element of `values`, charging (and possibly failing)
    /// before each growth, exactly like repeated [`ScratchVec::push`].
    ///
    /// # Errors
    /// Returns `BudgetExceeded(ScratchBytes)` before any allocation; elements
    /// already pushed before the failing one remain in place.
    pub fn extend(&mut self, values: impl IntoIterator<Item = T>) -> Result<(), StopReason> {
        for value in values {
            self.push(value)?;
        }
        Ok(())
    }

    /// Number of elements.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether there are no elements.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Borrow the elements.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.inner
    }

    /// Take ownership of a plain `Vec<T>`, permanently retaining the charge
    /// (it no longer describes transient scratch, so it is never released).
    #[must_use]
    pub fn into_vec(mut self) -> Vec<T> {
        let inner = mem::take(&mut self.inner);
        mem::forget(self);
        inner
    }
}

impl<T> Drop for ScratchVec<T> {
    fn drop(&mut self) {
        let _ = self.meter.release(self.reserved);
    }
}

/// A `String` whose growth is charged before it is allocated.
///
/// Growth is charged against a [`ScratchMeter`] before it is requested from
/// the allocator, and the charge is released exactly once, on drop. See
/// [`ScratchVec`] for the growth/charging contract.
///
/// ```
/// use purrassist_core::{Budget, Governor, ScratchString};
/// let mut governor = Governor::new(Budget::default());
/// let mut text = ScratchString::new(&governor);
/// text.push_str("hello").unwrap();
/// assert_eq!(text.as_str(), "hello");
/// let plain: String = text.into_string();
/// assert_eq!(plain, "hello");
/// ```
#[derive(Debug)]
pub struct ScratchString {
    inner: String,
    meter: ScratchMeter,
    reserved: u64,
}

impl ScratchString {
    /// An empty string charging against `governor`'s scratch meter.
    #[must_use]
    pub fn new(governor: &Governor) -> Self {
        Self {
            inner: String::new(),
            meter: governor.scratch_meter(),
            reserved: 0,
        }
    }

    /// Ensure capacity for `additional` more bytes, charging before reserving.
    ///
    /// # Errors
    /// Returns `BudgetExceeded(ScratchBytes)` before any allocation.
    pub fn reserve(&mut self, additional: usize) -> Result<(), StopReason> {
        let capacity = self.inner.capacity();
        let Some(target) = grown_capacity(self.inner.len(), capacity, additional) else {
            return Err(StopReason::BudgetExceeded(Resource::ScratchBytes));
        };
        if target == capacity {
            return Ok(());
        }
        let delta = size(target - capacity);
        self.meter.charge(delta)?;
        self.reserved = self.reserved.saturating_add(delta);
        if let Err(error) = self.inner.try_reserve_exact(target - self.inner.len()) {
            let _ = self.meter.release(delta);
            self.reserved = self.reserved.saturating_sub(delta);
            return Err(reserve_failure(&error));
        }
        Ok(())
    }

    /// Append `text`, growing (and charging) first if needed.
    ///
    /// # Errors
    /// Returns `BudgetExceeded(ScratchBytes)` before any allocation.
    pub fn push_str(&mut self, text: &str) -> Result<(), StopReason> {
        self.reserve(text.len())?;
        self.inner.push_str(text);
        Ok(())
    }

    /// Append one character, growing (and charging) first if needed.
    ///
    /// # Errors
    /// Returns `BudgetExceeded(ScratchBytes)` before any allocation.
    pub fn push(&mut self, character: char) -> Result<(), StopReason> {
        self.reserve(character.len_utf8())?;
        self.inner.push(character);
        Ok(())
    }

    /// Borrow the text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.inner
    }

    /// Number of bytes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether there are no bytes.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Take ownership of a plain `String`, permanently retaining the charge
    /// (it no longer describes transient scratch, so it is never released).
    #[must_use]
    pub fn into_string(mut self) -> String {
        let inner = mem::take(&mut self.inner);
        mem::forget(self);
        inner
    }
}

impl Drop for ScratchString {
    fn drop(&mut self) {
        let _ = self.meter.release(self.reserved);
    }
}

/// An ordered association list keyed by `K`, whose growth is charged.
///
/// Charged against a [`ScratchMeter`] before it is requested from the
/// allocator, released exactly once on drop. `purrassist-core` never uses
/// `HashMap` (its default hasher draws OS randomness, which the core's
/// ambient-access contract forbids) and `BTreeMap` has no fallible/charged
/// growth primitive in `std`, so `ScratchMap` is a sorted `Vec<(K, V)>`:
/// `O(log n)` lookup, `O(n)` insert. It is intended for the small, transient
/// maps a governed search builds and discards, not as a general associative
/// container.
///
/// ```
/// use purrassist_core::{Budget, Governor, ScratchMap};
/// let mut governor = Governor::new(Budget::default());
/// let mut seen: ScratchMap<u32, &'static str> = ScratchMap::new(&governor);
/// seen.insert(2, "two").unwrap();
/// seen.insert(1, "one").unwrap();
/// assert_eq!(seen.get(&1), Some(&"one"));
/// assert_eq!(seen.len(), 2);
/// ```
#[derive(Debug)]
pub struct ScratchMap<K, V> {
    inner: Vec<(K, V)>,
    meter: ScratchMeter,
    reserved: u64,
}

impl<K: Ord + ScratchCharge, V: ScratchCharge> ScratchMap<K, V> {
    /// An empty map charging against `governor`'s scratch meter.
    #[must_use]
    pub fn new(governor: &Governor) -> Self {
        Self {
            inner: Vec::new(),
            meter: governor.scratch_meter(),
            reserved: 0,
        }
    }

    fn position(&self, key: &K) -> Result<usize, usize> {
        self.inner.binary_search_by(|(entry, _)| entry.cmp(key))
    }

    /// Insert or replace the value for `key`, growing (and charging) the
    /// backing storage first if needed. Returns the replaced value, if any.
    ///
    /// # Errors
    /// Returns `BudgetExceeded(ScratchBytes)` before any allocation.
    pub fn insert(&mut self, key: K, value: V) -> Result<Option<V>, StopReason> {
        match self.position(&key) {
            Ok(index) => Ok(Some(mem::replace(&mut self.inner[index].1, value))),
            Err(index) => {
                if self.inner.len() == self.inner.capacity() {
                    self.reserve(1)?;
                }
                self.inner.insert(index, (key, value));
                Ok(None)
            }
        }
    }

    /// Ensure capacity for `additional` more entries, charging before
    /// reserving.
    ///
    /// # Errors
    /// Returns `BudgetExceeded(ScratchBytes)` before any allocation.
    pub fn reserve(&mut self, additional: usize) -> Result<(), StopReason> {
        let capacity = self.inner.capacity();
        let Some(target) = grown_capacity(self.inner.len(), capacity, additional) else {
            return Err(StopReason::BudgetExceeded(Resource::ScratchBytes));
        };
        if target == capacity {
            return Ok(());
        }
        let delta = size(target - capacity).saturating_mul(size(<(K, V)>::CHARGE.max(1)));
        self.meter.charge(delta)?;
        self.reserved = self.reserved.saturating_add(delta);
        if let Err(error) = self.inner.try_reserve_exact(target - self.inner.len()) {
            let _ = self.meter.release(delta);
            self.reserved = self.reserved.saturating_sub(delta);
            return Err(reserve_failure(&error));
        }
        Ok(())
    }

    /// Borrow the value for `key`.
    #[must_use]
    pub fn get(&self, key: &K) -> Option<&V> {
        self.position(key).ok().map(|index| &self.inner[index].1)
    }

    /// Remove and return the value for `key`.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let index = self.position(key).ok()?;
        Some(self.inner.remove(index).1)
    }

    /// Number of entries.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether there are no entries.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Iterate entries in key order.
    pub fn iter(&self) -> impl Iterator<Item = &(K, V)> {
        self.inner.iter()
    }

    /// Take ownership of the plain, key-ordered pairs, permanently retaining
    /// the charge (it no longer describes transient scratch, so it is never
    /// released).
    #[must_use]
    pub fn into_pairs(mut self) -> Vec<(K, V)> {
        let inner = mem::take(&mut self.inner);
        mem::forget(self);
        inner
    }
}

impl<K, V> Drop for ScratchMap<K, V> {
    fn drop(&mut self) {
        let _ = self.meter.release(self.reserved);
    }
}

/// A sticky governor: after exhaustion or cancellation, every charge fails.
///
/// `Governor` is [`Clone`], but cloning never shares live scratch accounting:
/// each clone gets its own independent [`ScratchMeter`] snapshot (matching
/// [`Usage`], which is plain data). Only a [`ScratchLease`]/[`ScratchVec`]/
/// [`ScratchString`]/[`ScratchMap`] issued *from the same governor instance*
/// shares its meter with that governor, so it keeps charging after the call
/// that created it returns. A cloned governor still shares an attached
/// [`CancellationToken`], because that is caller-owned, cross-cutting signal
/// state, not this governor's own accounting.
#[derive(Debug)]
pub struct Governor {
    limits: Budget,
    usage: Usage,
    scratch: ScratchMeter,
    stopped: Option<StopReason>,
    cancellation: Option<CancellationToken>,
}

impl Clone for Governor {
    fn clone(&self) -> Self {
        Self {
            limits: self.limits,
            usage: self.usage,
            scratch: self.scratch.snapshot(),
            stopped: self.stopped.clone(),
            cancellation: self.cancellation.clone(),
        }
    }
}

impl Governor {
    /// Start an operation with explicit limits.
    #[must_use]
    pub fn new(limits: Budget) -> Self {
        Self {
            limits,
            usage: Usage::default(),
            scratch: ScratchMeter::new(limits.scratch_bytes),
            stopped: None,
            cancellation: None,
        }
    }

    /// Start with explicit limits and a caller-owned cancellation signal.
    ///
    /// Usage and local exhaustion remain independent between governors sharing
    /// a token. An already recorded exhaustion retains precedence over a later
    /// cancellation request. Checks do not admit new work after cancellation.
    #[must_use]
    pub fn with_cancellation(limits: Budget, cancellation: CancellationToken) -> Self {
        Self {
            cancellation: Some(cancellation),
            ..Self::new(limits)
        }
    }

    /// The effective configured limits.
    #[must_use]
    pub const fn limits(&self) -> &Budget {
        &self.limits
    }

    /// Consumption measured so far.
    ///
    /// `peak_scratch_bytes` is read live from this governor's scratch meter,
    /// so it reflects charges made by a [`ScratchVec`]/[`ScratchString`]/
    /// [`ScratchMap`]/[`ScratchLease`] issued by this governor even though
    /// those charge the meter directly rather than through
    /// [`Governor::charge`].
    #[must_use]
    pub fn usage(&self) -> Usage {
        Usage {
            peak_scratch_bytes: self.scratch.peak(),
            total_scratch_charged: self.scratch.total_charged(),
            ..self.usage
        }
    }

    /// Cancel future work without changing completed evidence.
    pub fn cancel(&mut self) {
        self.stopped.get_or_insert(StopReason::Cancelled);
    }

    /// Check whether the operation can proceed.
    ///
    /// # Errors
    /// Returns the first exhaustion reason, or cancellation.
    pub fn check(&self) -> Result<(), StopReason> {
        if let Some(reason) = &self.stopped {
            return Err(reason.clone());
        }
        if self.scratch.is_exhausted() {
            // A scratch-aware collection issued by this governor (or by a
            // clone sharing this same meter) may have exhausted the shared
            // scratch dimension directly, without going through `charge`.
            return Err(StopReason::BudgetExceeded(Resource::ScratchBytes));
        }
        if self
            .cancellation
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            return Err(StopReason::Cancelled);
        }
        Ok(())
    }

    /// Charge before performing work or allocating reserved storage.
    /// `InputBytes` and `Depth` check a single admission, rather than accumulate.
    ///
    /// # Errors
    /// Returns a sticky exhaustion or cancellation without incrementing usage.
    pub fn charge(&mut self, resource: Resource, amount: u64) -> Result<(), StopReason> {
        self.check()?;
        if resource == Resource::ScratchBytes {
            self.scratch.charge(amount).inspect_err(|reason| {
                self.stopped = Some(reason.clone());
            })?;
            self.usage.peak_scratch_bytes = self.usage.peak_scratch_bytes.max(self.scratch.peak());
            return Ok(());
        }
        let (current, limit) = match resource {
            Resource::Work => (self.usage.work, self.limits.work),
            Resource::Candidates => (self.usage.candidates, self.limits.candidates),
            Resource::InputBytes => (0, self.limits.input_bytes),
            Resource::Depth => (0, u64::from(self.limits.depth)),
            Resource::EvidenceEntries => {
                (self.usage.evidence_entries, self.limits.evidence_entries)
            }
            Resource::HelperCalls => (self.usage.helper_calls, self.limits.helper_calls),
            Resource::ScratchBytes => unreachable!("handled above"),
        };
        let Some(next) = current.checked_add(amount).filter(|next| *next <= limit) else {
            let reason = StopReason::BudgetExceeded(resource);
            self.stopped = Some(reason.clone());
            return Err(reason);
        };
        match resource {
            Resource::Work => self.usage.work = next,
            Resource::Candidates => self.usage.candidates = next,
            Resource::EvidenceEntries => self.usage.evidence_entries = next,
            Resource::HelperCalls => self.usage.helper_calls = next,
            Resource::Depth | Resource::InputBytes => {}
            Resource::ScratchBytes => unreachable!("handled above"),
        }
        Ok(())
    }

    /// Release a previous scratch reservation.
    ///
    /// # Errors
    /// Rejects an unbalanced release, preserving the current reservation.
    pub fn release_scratch(&mut self, amount: u64) -> Result<(), StopReason> {
        self.scratch.release(amount)
    }

    /// Charge and hold `bytes` of scratch, released automatically when the
    /// returned lease is dropped (or permanently kept via
    /// [`ScratchLease::retain`]).
    ///
    /// # Errors
    /// Returns a sticky exhaustion or cancellation, or `BudgetExceeded`,
    /// before any allocation.
    pub fn lease_scratch(&mut self, bytes: u64) -> Result<ScratchLease, StopReason> {
        self.charge(Resource::ScratchBytes, bytes)?;
        Ok(ScratchLease::new(self.scratch.clone(), bytes))
    }

    /// Wrap `bytes` already charged by one or more prior calls to
    /// [`Governor::charge`] with [`Resource::ScratchBytes`] into a lease, so a
    /// loop that charges incrementally can still express "release all of this
    /// together" without a second charge.
    #[must_use]
    pub fn scratch_lease_for(&self, bytes: u64) -> ScratchLease {
        ScratchLease::new(self.scratch.clone(), bytes)
    }

    /// A clone of this governor's scratch meter, for constructing
    /// [`ScratchVec`]/[`ScratchString`]/[`ScratchMap`] collections that keep
    /// charging this governor's scratch budget after this call returns.
    #[must_use]
    pub fn scratch_meter(&self) -> ScratchMeter {
        self.scratch.clone()
    }
}

/// Exact source identity and a half-open byte range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSpan {
    /// Digest of the complete original source.
    pub source: Digest,
    /// Byte range in that source.
    pub bytes: Range<usize>,
}

/// One attributable correction or selection decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evidence {
    /// Stable rule identifier.
    pub rule: String,
    /// Explanation grounded in the performed operation.
    pub explanation: String,
    /// Exact original location when the operation has one.
    pub span: Option<SourceSpan>,
    /// Location in the result, usually a JSON Pointer or structural selector.
    pub destination: String,
}

/// Information removed or irreversibly transformed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loss {
    /// The rule responsible for the loss.
    pub evidence: Evidence,
    /// Original information, retained verbatim or as its exact source span.
    pub original: String,
}

/// How an output value was obtained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InferenceKind {
    /// Derived from identified input, schema, or domain evidence.
    Derived,
    /// Invented to construct a candidate satisfying constraints.
    Guess,
}

/// An inferred or invented result value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inference {
    /// Supporting evidence, including the result location.
    pub evidence: Evidence,
    /// Distinguishes grounding from speculation.
    pub kind: InferenceKind,
    /// The supplied result value in its documented representation.
    pub value: String,
}

/// Evidence of an actual validation performed on a candidate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Validation {
    /// Validator implementation and contract identity.
    pub validator: String,
    /// Constraint identity or human-readable constraint description.
    pub constraint: String,
    /// Whether the validation succeeded.
    pub passed: bool,
    /// Validation diagnostics.
    pub diagnostics: Vec<String>,
}

/// Exact source retained so transformation evidence can be replayed and inspected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceDocument {
    /// Identity of these exact UTF-8 bytes.
    pub digest: Digest,
    /// Complete source before recovery, including whitespace and malformed syntax.
    pub text: String,
}

/// Evidence belonging to one competing recovery or semantic interpretation.
///
/// Records follow the order of `Outcome::Ambiguous`. Their output identities hash
/// `Node::to_json()` bytes, not the original input or an equivalence-normalized JSON
/// value. Validation indices refer to the enclosing receipt's validation log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlternativeReceipt {
    /// Digest of this alternative's serialized JSON output.
    pub output: Digest,
    /// Recovery search cost; comparable within this operation only.
    pub cost: u64,
    /// Transformations that produced this alternative.
    pub evidence: Vec<Evidence>,
    /// Information discarded by this alternative alone.
    pub losses: Vec<Loss>,
    /// Derived or invented values belonging to this alternative.
    pub inferences: Vec<Inference>,
    /// Indices of validations actually performed on this output.
    pub validations: Vec<usize>,
}

/// An ordered, versioned audit receipt. Ledgers contain no hidden model reasoning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    /// Receipt representation version.
    pub version: u32,
    /// Original input identity.
    pub source: Digest,
    /// Retained recovery inputs, including helper proposal sources when applicable.
    /// Empty if admission failed before retention or this operation only ranks names.
    pub sources: Vec<SourceDocument>,
    /// Stable operation/profile identifier.
    pub operation: String,
    /// Requested effort preset.
    pub effort: Effort,
    /// Effective resource limits.
    pub budget: Budget,
    /// Measured consumption.
    pub usage: Usage,
    /// Recorded transformations and decisions in execution order.
    pub evidence: Vec<Evidence>,
    /// Discarded or irreversibly changed information.
    pub losses: Vec<Loss>,
    /// Derived and guessed output values.
    pub inferences: Vec<Inference>,
    /// Actual validation results.
    pub validations: Vec<Validation>,
    /// Per-alternative ledgers in outcome order for recovery, semantic, path,
    /// and embedding resolution. Empty for operations without alternative ledgers
    /// or a single answer. Mutually exclusive transformations stay here
    /// rather than being combined into the top-level selected-result ledgers.
    pub alternatives: Vec<AlternativeReceipt>,
    /// Why processing stopped.
    pub stopped: StopReason,
}

impl Receipt {
    /// Begin a receipt tied to exact source bytes and an explicit profile.
    #[must_use]
    pub fn new(source: &[u8], operation: &str, effort: Effort, budget: Budget) -> Self {
        Self {
            version: RECEIPT_VERSION,
            source: Digest::of(source),
            sources: Vec::new(),
            operation: operation.into(),
            effort,
            budget,
            usage: Usage::default(),
            evidence: Vec::new(),
            losses: Vec::new(),
            inferences: Vec::new(),
            validations: Vec::new(),
            alternatives: Vec::new(),
            stopped: StopReason::Completed,
        }
    }

    /// Retain exact input bytes under the operation's shared resource bounds.
    ///
    /// # Errors
    /// Returns exhaustion before copying the source. Rejected oversized input is
    /// identified by the receipt digest but must remain owned by the caller.
    pub fn retain_source(
        &mut self,
        source: &str,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let length = u64::try_from(source.len()).unwrap_or(u64::MAX);
        governor.charge(Resource::InputBytes, length)?;
        governor.charge(Resource::Work, length)?;
        governor.charge(Resource::ScratchBytes, length.saturating_add(128))?;
        self.sources.push(SourceDocument {
            digest: Digest::of(source.as_bytes()),
            text: source.into(),
        });
        Ok(())
    }

    /// Whether any selected result or competing alternative was explicitly guessed.
    #[must_use]
    pub fn contains_guesses(&self) -> bool {
        self.inferences
            .iter()
            .any(|entry| entry.kind == InferenceKind::Guess)
            || self.alternatives.iter().any(|alternative| {
                alternative
                    .inferences
                    .iter()
                    .any(|entry| entry.kind == InferenceKind::Guess)
            })
    }
}

/// An answer's epistemic status, independently of schema or syntax validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome<T> {
    /// A preferred result supported by the recorded evidence.
    Resolved(T),
    /// A constraint-satisfying answer containing speculative information.
    BestGuess(T),
    /// Competing candidates require caller disambiguation.
    Ambiguous(Vec<T>),
    /// No acceptable answer was obtained within the operation's contract.
    Unresolved,
}

/// The answer together with its evidence; receipts also accompany failures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssistResult<T> {
    /// Result and its epistemic status.
    pub outcome: Outcome<T>,
    /// Source provenance, corrections, losses, inferences, and validation.
    pub receipt: Receipt,
}

#[cfg(test)]
mod tests {
    use super::{Budget, Governor, Resource, StopReason, Usage};

    #[test]
    fn usage_merge_sums_counters_and_maxes_the_high_water_mark() {
        let first = Usage {
            work: 10,
            candidates: 3,
            peak_scratch_bytes: 100,
            total_scratch_charged: 150,
            evidence_entries: 2,
            helper_calls: 1,
        };
        let second = Usage {
            work: 5,
            candidates: 7,
            peak_scratch_bytes: 40,
            total_scratch_charged: 40,
            evidence_entries: 4,
            helper_calls: 0,
        };
        let merged = first.merge(&second);
        assert_eq!(merged.work, 15);
        assert_eq!(merged.candidates, 10);
        assert_eq!(merged.peak_scratch_bytes, 100); // max, not sum: never simultaneously live.
        assert_eq!(merged.total_scratch_charged, 190); // sum: a monotone cumulative counter.
        assert_eq!(merged.evidence_entries, 6);
        assert_eq!(merged.helper_calls, 1);
        // Merging is commutative for every field used here (sums and max both are).
        assert_eq!(merged, second.merge(&first));
        // Merging with a default identity changes nothing.
        assert_eq!(first.merge(&Usage::default()), first);
    }

    #[test]
    fn exhaustion_is_sticky_and_never_overcharges() {
        let mut governor = Governor::new(Budget {
            work: 2,
            ..Budget::default()
        });
        assert_eq!(governor.charge(Resource::Work, 2), Ok(()));
        let failure = Err(StopReason::BudgetExceeded(Resource::Work));
        assert_eq!(governor.charge(Resource::Work, 1), failure);
        assert_eq!(governor.charge(Resource::Candidates, 0), failure);
        assert_eq!(governor.usage().work, 2);
    }

    #[test]
    fn scratch_tracks_live_and_high_water_usage() {
        let mut governor = Governor::new(Budget {
            scratch_bytes: 10,
            ..Budget::default()
        });
        assert_eq!(governor.charge(Resource::ScratchBytes, 8), Ok(()));
        assert_eq!(governor.release_scratch(8), Ok(()));
        assert_eq!(governor.charge(Resource::ScratchBytes, 7), Ok(()));
        assert_eq!(governor.usage().peak_scratch_bytes, 8);
        assert!(governor.release_scratch(9).is_err());
        assert!(governor.charge(Resource::ScratchBytes, 4).is_err());
    }

    #[test]
    fn scratch_vec_charges_before_growing_and_releases_on_drop() {
        let mut governor = Governor::new(Budget::default());
        {
            let mut values: super::ScratchVec<u64> = super::ScratchVec::new(&governor);
            values.push(1).unwrap();
            assert!(governor.usage().peak_scratch_bytes > 0);
            values.push(2).unwrap();
            values.push(3).unwrap();
            assert_eq!(values.as_slice(), &[1, 2, 3]);
        }
        assert_eq!(governor.charge(Resource::ScratchBytes, 0), Ok(()));
        // Every byte the vector charged came back: a fresh charge for the
        // whole (tiny) budget succeeds only if nothing leaked.
        let remaining = governor.limits().scratch_bytes;
        assert_eq!(governor.charge(Resource::ScratchBytes, remaining), Ok(()));
    }

    #[test]
    fn scratch_vec_into_vec_permanently_retains_its_charge() {
        let mut governor = Governor::new(Budget::default());
        let mut values: super::ScratchVec<u64> = super::ScratchVec::new(&governor);
        values.push(1).unwrap();
        let charged = governor.usage().peak_scratch_bytes;
        assert!(charged > 0);
        let plain = values.into_vec();
        assert_eq!(plain, vec![1]);
        // The charge is not released by `into_vec`, unlike a normal drop:
        // exactly filling the remaining budget succeeds, and one more byte
        // does not fit.
        let remaining = governor.limits().scratch_bytes;
        assert_eq!(
            governor.charge(Resource::ScratchBytes, remaining - charged),
            Ok(())
        );
        assert!(governor.charge(Resource::ScratchBytes, 1).is_err());
    }

    #[test]
    fn scratch_string_and_map_charge_and_release_precisely() {
        let mut governor = Governor::new(Budget::default());
        {
            let mut text = super::ScratchString::new(&governor);
            text.push_str("hello").unwrap();
            text.push(' ').unwrap();
            text.push_str("world").unwrap();
            assert_eq!(text.as_str(), "hello world");
        }
        {
            let mut map: super::ScratchMap<u32, &'static str> = super::ScratchMap::new(&governor);
            assert_eq!(map.insert(2, "two").unwrap(), None);
            assert_eq!(map.insert(1, "one").unwrap(), None);
            assert_eq!(map.insert(1, "uno").unwrap(), Some("one"));
            assert_eq!(map.get(&1), Some(&"uno"));
            assert_eq!(map.len(), 2);
            assert_eq!(map.remove(&2), Some("two"));
            assert_eq!(map.len(), 1);
        }
        let remaining = governor.limits().scratch_bytes;
        assert_eq!(governor.charge(Resource::ScratchBytes, remaining), Ok(()));
    }

    #[test]
    fn scratch_lease_releases_on_drop_and_retain_keeps_the_charge() {
        let mut governor = Governor::new(Budget::default());
        {
            let lease = governor.lease_scratch(64).unwrap();
            assert_eq!(lease.bytes(), 64);
        }
        assert_eq!(governor.usage().peak_scratch_bytes, 64);
        let remaining = governor.limits().scratch_bytes;
        assert_eq!(governor.charge(Resource::ScratchBytes, remaining), Ok(()));
        let mut governor = Governor::new(Budget::default());
        governor.lease_scratch(64).unwrap().retain();
        let remaining = governor.limits().scratch_bytes;
        assert!(governor.charge(Resource::ScratchBytes, remaining).is_err());
    }

    #[test]
    fn scratch_exhaustion_from_a_collection_is_sticky_on_the_governor() {
        let mut governor = Governor::new(Budget {
            scratch_bytes: 4,
            ..Budget::default()
        });
        let mut values: super::ScratchVec<u64> = super::ScratchVec::new(&governor);
        // Grows to capacity 4, charging 4 * size_of::<u64>() = 32 bytes: over budget.
        assert!(values.push(1).is_err());
        drop(values);
        // The meter is sticky-exhausted even though `governor.charge` was
        // never called directly; every future charge, on any resource, fails.
        assert!(governor.check().is_err());
        assert_eq!(
            governor.charge(Resource::Work, 0),
            Err(StopReason::BudgetExceeded(Resource::ScratchBytes))
        );
    }

    #[test]
    fn cloned_governors_have_independent_scratch_accounting() {
        let mut governor = Governor::new(Budget::default());
        governor.charge(Resource::ScratchBytes, 100).unwrap();
        let mut clone = governor.clone();
        clone.charge(Resource::ScratchBytes, 50).unwrap();
        assert_eq!(governor.usage().peak_scratch_bytes, 100);
        assert_eq!(clone.usage().peak_scratch_bytes, 150);
        governor.release_scratch(100).unwrap();
        // Releasing on the original does not affect the independent clone.
        assert!(clone.release_scratch(200).is_err());
        assert_eq!(clone.release_scratch(150), Ok(()));
    }
}
