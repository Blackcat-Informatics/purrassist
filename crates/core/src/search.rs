// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Generic, governed best-first search with a total deterministic order.
//!
//! A [`Search`] explores [`SearchState`] values in ascending
//! `(cost, tie, serial)` order. `cost` is the accumulated [`Ledger::cost`],
//! `tie` is the state's own [`SearchState::Tie`] key, and `serial` is the
//! enqueue sequence number. The tie key makes the order total and independent
//! of the order in which [`SearchState::expand`] emits equal-cost proposals;
//! a state whose tie type is `()` keeps plain `(cost, serial)` order.
//!
//! Every candidate is deduplicated by the canonical key its state writes with
//! [`SearchState::write_key`]. A duplicate at an equal or higher cost is never
//! queued; a cheaper duplicate is queued and the older, now-stale entry is
//! skipped (neither accepted nor expanded) when it is eventually popped.
//!
//! The search stops at the first accepted cost: every accepted candidate of
//! that minimum cost is kept, and [`Search::finish`] reports one as
//! `Resolved` (or `BestGuess` when its ledger carries inferences), several as
//! `Ambiguous` through [`Forest::publish`], and none as `Unresolved` with
//! [`StopReason::NoCandidate`].
//!
//! # Resource contract
//!
//! Every enqueued candidate charges `Work` for its key length, one
//! `Candidates` unit (none for a [`Search::uncounted`] search), and
//! `key_len * 32 + 256` scratch bytes. The part of that
//! scratch charge backing the queue entry is released when the entry is
//! popped (or when the search is dropped); the part backing the retained
//! deduplication key (`key_len + 64`, once per distinct key) is released when
//! the [`Search`] is dropped. [`Search::seed_with`] checks a seed's key and
//! cost before building it, so a duplicate charges only its key's `Work`.
//! Exhaustion or cancellation is returned as `Err` and never becomes an
//! accepted answer.
use crate::contract::saturating_size as size;
use crate::contract::{
    Evidence, Governor, Inference, InferenceKind, Loss, Outcome, Receipt, Resource, ScratchLease,
    ScratchMeter, ScratchVec, StopReason,
};
use crate::digest::Digest;
use crate::provenance::{Derivation, Forest, permute};
use core::fmt;
use std::collections::BTreeMap;

/// Scratch bytes charged per enqueued candidate on top of `32` per key byte.
const ENTRY_OVERHEAD: u64 = 256;
/// Scratch bytes per retained deduplication key on top of its own length.
const SEEN_OVERHEAD: u64 = 64;

/// One state explored by a [`Search`].
///
/// Implementations supply the canonical deduplication key, the total-order
/// tie-break key, the acceptance test, and the expansion step. The
/// [`SearchState::Context`] carries whatever shared, mutable search-wide
/// state acceptance and expansion need (a validation log, a schema, a
/// request); the search itself never inspects it.
///
/// ```
/// use purrassist_core::search::{Acceptance, Proposals, SearchState};
/// use purrassist_core::{Governor, ScratchVec, StopReason};
///
/// #[derive(Clone)]
/// struct Count(u8);
/// impl SearchState for Count {
///     type Tie = u8;
///     type Context<'c> = ();
///     fn tie(&self) -> u8 {
///         self.0
///     }
///     fn write_key(&self, key: &mut ScratchVec<u8>) -> Result<(), StopReason> {
///         key.push(self.0)
///     }
///     fn accept(&self, _: &mut (), _: &mut Governor) -> Result<Acceptance, StopReason> {
///         Ok(if self.0 == 3 {
///             Acceptance::Accepted { validation: 0 }
///         } else {
///             Acceptance::Rejected
///         })
///     }
///     fn expand(
///         &self,
///         _: &mut (),
///         _: &mut Proposals<'_, Self>,
///         _: &mut Governor,
///     ) -> Result<(), StopReason> {
///         Ok(())
///     }
/// }
/// assert_eq!(Count(3).tie(), 3);
/// ```
pub trait SearchState: Sized {
    /// Total-order tie-break key among equal-cost candidates. It must be
    /// derived only from the state itself, never from proposal order.
    type Tie: Ord;
    /// Shared, mutable search-wide state passed to acceptance and expansion.
    type Context<'c>;

    /// This state's tie-break key.
    fn tie(&self) -> Self::Tie;

    /// Write this state's canonical deduplication key into `key`, which is
    /// empty on entry and charged against the search's governor as it grows.
    ///
    /// # Errors
    /// Returns `BudgetExceeded(ScratchBytes)` when the key buffer cannot grow.
    fn write_key(&self, key: &mut ScratchVec<u8>) -> Result<(), StopReason>;

    /// Write the bytes whose digest identifies this state as an answer in a
    /// published derivation. Defaults to the canonical deduplication key.
    ///
    /// # Errors
    /// Returns `BudgetExceeded(ScratchBytes)` when the buffer cannot grow.
    fn write_output(&self, output: &mut ScratchVec<u8>) -> Result<(), StopReason> {
        self.write_key(output)
    }

    /// Decide whether this state is an acceptable answer.
    ///
    /// # Errors
    /// Returns resource exhaustion or cancellation; the search stops and the
    /// error propagates unchanged.
    fn accept(
        &self,
        context: &mut Self::Context<'_>,
        governor: &mut Governor,
    ) -> Result<Acceptance, StopReason>;

    /// Propose successor states through [`Proposals::commit`]. Called only
    /// for rejected states, and only before any state has been accepted.
    ///
    /// # Errors
    /// Returns resource exhaustion or cancellation; the search stops and the
    /// error propagates unchanged.
    fn expand(
        &self,
        context: &mut Self::Context<'_>,
        proposals: &mut Proposals<'_, Self>,
        governor: &mut Governor,
    ) -> Result<(), StopReason>;
}

/// The verdict of [`SearchState::accept`].
///
/// ```
/// use purrassist_core::search::Acceptance;
/// let verdict = Acceptance::Accepted { validation: 2 };
/// assert_ne!(verdict, Acceptance::Rejected);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Acceptance {
    /// The state is an acceptable answer; `validation` indexes the passing
    /// entry in the caller's receipt validation log that proves it.
    Accepted {
        /// Index into `Receipt::validations`.
        validation: usize,
    },
    /// The state is not an acceptable answer and may be expanded.
    Rejected,
}

/// One recorded transformation applied while deriving a candidate.
///
/// `evidence` names the rule, the source span and destination, and the
/// explanation. `original` is the discarded original text, recorded as a
/// [`Loss`]; `guess` is the invented value, recorded as a
/// [`InferenceKind::Guess`] inference (its presence is the guessed flag).
///
/// ```
/// use purrassist_core::contract::Evidence;
/// use purrassist_core::search::{Change, Ledger};
/// let mut ledger = Ledger::default();
/// ledger.record(Change {
///     evidence: Evidence {
///         rule: "demo.rule".into(),
///         explanation: "replace with 1".into(),
///         span: None,
///         destination: "/x".into(),
///     },
///     cost: 3,
///     original: Some("\"one\"".into()),
///     guess: None,
/// });
/// assert_eq!((ledger.cost, ledger.evidence.len(), ledger.losses.len()), (3, 1, 1));
/// assert!(ledger.inferences.is_empty());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// The rule, explanation, span, and destination of this change.
    pub evidence: Evidence,
    /// Search cost added by this change.
    pub cost: u64,
    /// Original text discarded or irreversibly changed, if any.
    pub original: Option<String>,
    /// Invented value when the change is a guess.
    pub guess: Option<String>,
}

/// Accumulated cost and provenance of one candidate.
///
/// ```
/// use purrassist_core::search::Ledger;
/// let ledger = Ledger::default();
/// assert_eq!(ledger.cost, 0);
/// assert_eq!(ledger.estimated_bytes(), 0);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ledger {
    /// Total search cost.
    pub cost: u64,
    /// Every recorded decision, in application order.
    pub evidence: Vec<Evidence>,
    /// Every discarded original, in application order.
    pub losses: Vec<Loss>,
    /// Every invented value, in application order.
    pub inferences: Vec<Inference>,
}

impl Ledger {
    /// Append `change`: its evidence, its loss when `original` is present,
    /// and a guess inference when `guess` is present. The cost saturates.
    pub fn record(&mut self, change: Change) {
        let Change {
            evidence,
            cost,
            original,
            guess,
        } = change;
        self.cost = self.cost.saturating_add(cost);
        self.evidence.push(evidence.clone());
        if let Some(original) = original {
            self.losses.push(Loss {
                evidence: evidence.clone(),
                original,
            });
        }
        if let Some(value) = guess {
            self.inferences.push(Inference {
                evidence,
                kind: InferenceKind::Guess,
                value,
            });
        }
    }

    /// A conservative scratch estimate for cloning this ledger's entries:
    /// 256 bytes per evidence, loss, and inference.
    #[must_use]
    pub fn estimated_bytes(&self) -> u64 {
        size(self.evidence.len())
            .saturating_add(size(self.losses.len()))
            .saturating_add(size(self.inferences.len()))
            .saturating_mul(256)
    }
}

struct Entry<S> {
    state: S,
    ledger: Ledger,
    key: Vec<u8>,
    held: ScratchLease,
}

/// A best-first search queue with a total deterministic order.
///
/// See the [module documentation](self) for the ordering, deduplication and
/// resource contract.
///
/// ```
/// use purrassist_core::search::{Acceptance, Ledger, Proposals, Search, SearchState};
/// use purrassist_core::contract::{Evidence, Receipt};
/// use purrassist_core::search::Change;
/// use purrassist_core::{Budget, Effort, Governor, Outcome, ScratchVec, StopReason};
///
/// /// Count up from 0; 3 is the only acceptable value.
/// #[derive(Clone, Debug, PartialEq)]
/// struct Count(u8);
/// impl SearchState for Count {
///     type Tie = u8;
///     type Context<'c> = Vec<u8>;
///     fn tie(&self) -> u8 {
///         self.0
///     }
///     fn write_key(&self, key: &mut ScratchVec<u8>) -> Result<(), StopReason> {
///         key.push(self.0)
///     }
///     fn accept(&self, seen: &mut Vec<u8>, _: &mut Governor) -> Result<Acceptance, StopReason> {
///         seen.push(self.0);
///         Ok(if self.0 == 3 {
///             Acceptance::Accepted { validation: 0 }
///         } else {
///             Acceptance::Rejected
///         })
///     }
///     fn expand(
///         &self,
///         _: &mut Vec<u8>,
///         proposals: &mut Proposals<'_, Self>,
///         governor: &mut Governor,
///     ) -> Result<(), StopReason> {
///         let evidence = Evidence {
///             rule: "count.up".into(),
///             explanation: "add one".into(),
///             span: None,
///             destination: String::new(),
///         };
///         let change = Change { evidence, cost: 1, original: None, guess: None };
///         proposals.commit(governor, Count(self.0 + 1), change)
///     }
/// }
///
/// let mut governor = Governor::new(Budget::default());
/// let mut receipt = Receipt::new(b"0", "count.v1", Effort::Standard, Budget::default());
/// let mut search = Search::new(&governor);
/// search.seed(&mut governor, Count(0), Ledger::default())?;
/// let mut seen = Vec::new();
/// let accepted = search.run(&mut seen, &mut governor)?;
/// let outcome = search.finish(accepted, &mut receipt, &mut governor)?;
/// assert_eq!(outcome, Outcome::Resolved(Count(3)));
/// assert_eq!(seen, [0, 1, 2, 3]);
/// assert_eq!(receipt.evidence.len(), 3);
/// # Ok::<(), StopReason>(())
/// ```
pub struct Search<S: SearchState> {
    queue: BTreeMap<(u64, S::Tie, u64), Entry<S>>,
    seen: BTreeMap<Vec<u8>, u64>,
    key: ScratchVec<u8>,
    serial: u64,
    meter: ScratchMeter,
    retained: u64,
    /// Whether each enqueued candidate charges one `Candidates` unit.
    counted: bool,
}

/// The minimum-cost accepted candidates of one [`Search::run`], in
/// acceptance order, ready for [`Search::finish`].
pub struct Accepted<S> {
    candidates: Vec<(S, Ledger, usize)>,
}

impl<S> Accepted<S> {
    /// Number of accepted candidates.
    ///
    /// ```
    /// # use purrassist_core::search::Accepted;
    /// fn empty(accepted: &Accepted<u8>) -> bool { accepted.len() == 0 }
    /// ```
    #[must_use]
    pub const fn len(&self) -> usize {
        self.candidates.len()
    }

    /// Whether nothing was accepted.
    ///
    /// ```
    /// # use purrassist_core::search::Accepted;
    /// fn none(accepted: &Accepted<u8>) -> bool { accepted.is_empty() }
    /// ```
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    /// The accepted states and their ledgers, in acceptance order, for a
    /// caller that uses the minimum-cost states directly instead of
    /// publishing them through [`Search::finish`].
    ///
    /// ```
    /// # use purrassist_core::search::{Accepted, Ledger};
    /// fn costs(accepted: Accepted<u8>) -> Vec<u64> {
    ///     accepted.into_states().into_iter().map(|(_, ledger)| ledger.cost).collect()
    /// }
    /// ```
    #[must_use]
    pub fn into_states(self) -> Vec<(S, Ledger)> {
        self.candidates
            .into_iter()
            .map(|(state, ledger, _)| (state, ledger))
            .collect()
    }
}

/// The successor sink handed to [`SearchState::expand`].
///
/// Every committed successor inherits a clone of the parent's [`Ledger`]
/// with one more [`Change`], and is enqueued immediately under the search's
/// ordering and deduplication rules.
pub struct Proposals<'p, S: SearchState> {
    search: &'p mut Search<S>,
    parent: &'p Ledger,
}

impl<S: SearchState> Proposals<'_, S> {
    /// The ledger of the state being expanded.
    ///
    /// ```
    /// # use purrassist_core::search::{Proposals, SearchState};
    /// fn parent_cost<S: SearchState>(proposals: &Proposals<'_, S>) -> u64 {
    ///     proposals.parent().cost
    /// }
    /// ```
    #[must_use]
    pub const fn parent(&self) -> &Ledger {
        self.parent
    }

    /// Enqueue `state` with the parent's ledger plus `change`.
    ///
    /// See the [`Search`] example.
    ///
    /// # Errors
    /// Returns exhaustion of `Work`, `Candidates`, or `ScratchBytes`, or
    /// cancellation, before the candidate is queued.
    pub fn commit(
        &mut self,
        governor: &mut Governor,
        state: S,
        change: Change,
    ) -> Result<(), StopReason> {
        let mut ledger = self.parent.clone();
        ledger.record(change);
        self.search.enqueue(governor, state, ledger)
    }
}

impl<S: SearchState> Search<S> {
    /// An empty search charging `governor`'s scratch meter.
    ///
    /// See the [`Search`] example.
    #[must_use]
    pub fn new(governor: &Governor) -> Self {
        Self {
            queue: BTreeMap::new(),
            seen: BTreeMap::new(),
            key: ScratchVec::new(governor),
            serial: 0,
            meter: governor.scratch_meter(),
            retained: 0,
            counted: true,
        }
    }

    /// An empty search whose queue charges no `Candidates`: every other
    /// charge is [`Search::new`]'s, so `Work` and `ScratchBytes` still bound
    /// it. For an internal exploration whose accepted states are not yet
    /// competing answers (the grammar engine's repair of a stuck dialect
    /// attempt): the caller charges `Candidates` for what survives to
    /// selection instead.
    ///
    /// ```
    /// use purrassist_core::search::{Acceptance, Change, Proposals, Search, SearchState};
    /// use purrassist_core::{Budget, Governor, ScratchVec, StopReason, search::Ledger};
    /// #[derive(Clone)]
    /// struct Done;
    /// impl SearchState for Done {
    ///     type Tie = ();
    ///     type Context<'c> = ();
    ///     fn tie(&self) {}
    ///     fn write_key(&self, key: &mut ScratchVec<u8>) -> Result<(), StopReason> { key.push(0) }
    ///     fn accept(&self, _: &mut (), _: &mut Governor) -> Result<Acceptance, StopReason> {
    ///         Ok(Acceptance::Accepted { validation: 0 })
    ///     }
    ///     fn expand(&self, _: &mut (), _: &mut Proposals<'_, Self>, _: &mut Governor) -> Result<(), StopReason> { Ok(()) }
    /// }
    /// let mut governor = Governor::new(Budget { candidates: 0, ..Budget::default() });
    /// let mut search = Search::uncounted(&governor);
    /// search.seed(&mut governor, Done, Ledger::default())?;
    /// assert_eq!(search.run(&mut (), &mut governor)?.into_states().len(), 1);
    /// assert_eq!(governor.usage().candidates, 0);
    /// # Ok::<(), StopReason>(())
    /// ```
    #[must_use]
    pub fn uncounted(governor: &Governor) -> Self {
        let mut search = Self::new(governor);
        search.counted = false;
        search
    }

    /// Enqueue an initial state with its own already-built ledger.
    ///
    /// See the [`Search`] example.
    ///
    /// # Errors
    /// Returns exhaustion of `Work`, `Candidates`, or `ScratchBytes`, or
    /// cancellation, before the candidate is queued.
    pub fn seed(
        &mut self,
        governor: &mut Governor,
        state: S,
        ledger: Ledger,
    ) -> Result<(), StopReason> {
        self.enqueue(governor, state, ledger)
    }

    /// Enqueue an initial state whose ledger cost is known in advance, but
    /// build the state and its ledger only if the candidate is admitted.
    ///
    /// `write_key` writes the candidate's canonical key (the same bytes its
    /// [`SearchState::write_key`] would write). A key already queued at a
    /// cost no greater than `cost` is a duplicate: `build` is never called,
    /// nothing beyond the key's `Work` is charged, and `Ok(false)` is
    /// returned. Otherwise `build` constructs the state and ledger, which
    /// are queued exactly as by [`Search::seed`], and `Ok(true)` is
    /// returned. Use this when building a ledger costs more than the key.
    ///
    /// # Errors
    /// Returns any error from `write_key` or `build`, exhaustion of `Work`,
    /// `Candidates`, or `ScratchBytes`, or cancellation, before the candidate
    /// is queued; and [`StopReason::InvalidInput`] if the built ledger's cost
    /// differs from `cost`.
    ///
    /// ```
    /// use purrassist_core::search::{Acceptance, Ledger, Proposals, Search, SearchState};
    /// use purrassist_core::{Budget, Governor, ScratchVec, StopReason};
    ///
    /// struct Word(&'static str);
    /// impl SearchState for Word {
    ///     type Tie = ();
    ///     type Context<'c> = ();
    ///     fn tie(&self) {}
    ///     fn write_key(&self, key: &mut ScratchVec<u8>) -> Result<(), StopReason> {
    ///         key.extend(self.0.bytes())
    ///     }
    ///     fn accept(&self, _: &mut (), _: &mut Governor) -> Result<Acceptance, StopReason> {
    ///         Ok(Acceptance::Accepted { validation: 0 })
    ///     }
    ///     fn expand(&self, _: &mut (), _: &mut Proposals<'_, Self>, _: &mut Governor) -> Result<(), StopReason> {
    ///         Ok(())
    ///     }
    /// }
    /// let mut governor = Governor::new(Budget::default());
    /// let mut search = Search::new(&governor);
    /// let build = |_: &mut Governor| Ok((Word("hi"), Ledger::default()));
    /// let key = |key: &mut ScratchVec<u8>| key.extend(*b"hi");
    /// assert_eq!(search.seed_with(&mut governor, 0, key, build), Ok(true));
    /// // The duplicate is rejected before its ledger would be built.
    /// let never = |_: &mut Governor| -> Result<(Word, Ledger), StopReason> { unreachable!() };
    /// assert_eq!(search.seed_with(&mut governor, 0, key, never), Ok(false));
    /// assert_eq!(governor.usage().candidates, 1);
    /// ```
    pub fn seed_with(
        &mut self,
        governor: &mut Governor,
        cost: u64,
        write_key: impl FnOnce(&mut ScratchVec<u8>) -> Result<(), StopReason>,
        build: impl FnOnce(&mut Governor) -> Result<(S, Ledger), StopReason>,
    ) -> Result<bool, StopReason> {
        if !self.admits(governor, cost, write_key)? {
            return Ok(false);
        }
        let (state, ledger) = build(governor)?;
        if ledger.cost != cost {
            return Err(StopReason::InvalidInput(format!(
                "seeded ledger cost {} differs from its admitted cost {cost}",
                ledger.cost
            )));
        }
        self.insert(governor, state, ledger)?;
        Ok(true)
    }

    fn enqueue(
        &mut self,
        governor: &mut Governor,
        state: S,
        ledger: Ledger,
    ) -> Result<(), StopReason> {
        if self.admits(governor, ledger.cost, |key| state.write_key(key))? {
            self.insert(governor, state, ledger)?;
        }
        Ok(())
    }

    /// Write a candidate's key into the reusable key buffer, charge its
    /// `Work`, and report whether it is not a duplicate at `cost` or less.
    fn admits(
        &mut self,
        governor: &mut Governor,
        cost: u64,
        write_key: impl FnOnce(&mut ScratchVec<u8>) -> Result<(), StopReason>,
    ) -> Result<bool, StopReason> {
        self.key.clear();
        write_key(&mut self.key)?;
        governor.charge(Resource::Work, size(self.key.len()))?;
        Ok(self
            .seen
            .get(self.key.as_slice())
            .is_none_or(|known| *known > cost))
    }

    /// Queue an admitted candidate whose key is in the key buffer.
    fn insert(
        &mut self,
        governor: &mut Governor,
        state: S,
        ledger: Ledger,
    ) -> Result<(), StopReason> {
        let key = self.key.as_slice();
        let known = self.seen.contains_key(key);
        if self.counted {
            governor.charge(Resource::Candidates, 1)?;
        }
        let total = size(key.len())
            .saturating_mul(32)
            .saturating_add(ENTRY_OVERHEAD);
        governor.charge(Resource::ScratchBytes, total)?;
        let retained = if known {
            0
        } else {
            size(key.len()).saturating_add(SEEN_OVERHEAD)
        };
        self.retained = self.retained.saturating_add(retained);
        let held = governor.scratch_lease_for(total - retained);
        let key = key.to_vec();
        self.seen.insert(key.clone(), ledger.cost);
        self.queue.insert(
            (ledger.cost, state.tie(), self.serial),
            Entry {
                state,
                ledger,
                key,
                held,
            },
        );
        self.serial += 1;
        Ok(())
    }

    /// Pop candidates in order until the first accepted cost is exhausted.
    ///
    /// Stale entries (a cheaper duplicate was queued later) are skipped
    /// without acceptance or expansion. Rejected candidates are expanded only
    /// while nothing has been accepted. See the [`Search`] example.
    ///
    /// # Errors
    /// Returns any error from acceptance or expansion, including resource
    /// exhaustion and cancellation, unchanged.
    pub fn run(
        &mut self,
        context: &mut S::Context<'_>,
        governor: &mut Governor,
    ) -> Result<Accepted<S>, StopReason> {
        let mut candidates = Vec::new();
        let mut winning_cost = None;
        while let Some(((cost, _, _), entry)) = self.queue.pop_first() {
            let Entry {
                state,
                ledger,
                key,
                held,
            } = entry;
            drop(held);
            if winning_cost.is_some_and(|winner| cost > winner) {
                break;
            }
            if self.seen.get(&key).is_some_and(|best| *best < cost) {
                continue;
            }
            match state.accept(context, governor)? {
                Acceptance::Accepted { validation } => {
                    winning_cost = Some(cost);
                    candidates.push((state, ledger, validation));
                }
                Acceptance::Rejected if winning_cost.is_none() => {
                    let mut proposals = Proposals {
                        search: self,
                        parent: &ledger,
                    };
                    state.expand(context, &mut proposals, governor)?;
                }
                Acceptance::Rejected => {}
            }
        }
        Ok(Accepted { candidates })
    }

    /// Publish `accepted` into `receipt` and classify the outcome.
    ///
    /// No candidate yields `Unresolved` with `receipt.stopped` set to
    /// [`StopReason::NoCandidate`]. One candidate appends its ledgers to the
    /// receipt and is `BestGuess` when it carries inferences, otherwise
    /// `Resolved`. Several candidates are interned into a [`Forest`]; shared
    /// provenance replaces the receipt's top-level ledgers, the remaining
    /// per-candidate ledgers become `receipt.alternatives`, and the
    /// `Ambiguous` values share the published order. See the [`Search`]
    /// example.
    ///
    /// # Errors
    /// Returns resource exhaustion or cancellation before `receipt` is
    /// modified for an ambiguous result; nothing partial is published.
    pub fn finish(
        &mut self,
        accepted: Accepted<S>,
        receipt: &mut Receipt,
        governor: &mut Governor,
    ) -> Result<Outcome<S>, StopReason>
    where
        S: Clone,
    {
        let mut accepted = accepted.candidates;
        if accepted.len() > 1 {
            return self.publish(accepted, receipt, governor);
        }
        let Some((state, ledger, _validation)) = accepted.pop() else {
            receipt.stopped = StopReason::NoCandidate;
            return Ok(Outcome::Unresolved);
        };
        let guessed = !ledger.inferences.is_empty();
        receipt.evidence.extend(ledger.evidence);
        receipt.losses.extend(ledger.losses);
        receipt.inferences.extend(ledger.inferences);
        Ok(if guessed {
            Outcome::BestGuess(state)
        } else {
            Outcome::Resolved(state)
        })
    }

    /// Route several equal-cost answers through the derivation forest.
    ///
    /// Provenance reachable from every accepted candidate is promoted once
    /// to the shared top-level ledgers. Nothing is admitted to `receipt`
    /// until every candidate is interned and the whole forest is published,
    /// so exhaustion never leaves a partial result.
    fn publish(
        &mut self,
        accepted: Vec<(S, Ledger, usize)>,
        receipt: &mut Receipt,
        governor: &mut Governor,
    ) -> Result<Outcome<S>, StopReason>
    where
        S: Clone,
    {
        let mut forest = Forest::new(governor);
        let mut derivations = Vec::with_capacity(accepted.len());
        let mut states = Vec::with_capacity(accepted.len());
        for (state, ledger, validation) in accepted {
            self.key.clear();
            state.write_output(&mut self.key)?;
            governor.charge(Resource::Work, size(self.key.len()))?;
            let mut derivation = Derivation::new(Digest::of(self.key.as_slice()), ledger.cost);
            for evidence in ledger.evidence {
                derivation.push(forest.evidence(governor, evidence)?);
            }
            for loss in ledger.losses {
                derivation.push(forest.loss(governor, loss)?);
            }
            for inference in ledger.inferences {
                derivation.push(forest.inference(governor, inference)?);
            }
            derivation.push(forest.validation(governor, validation, true)?);
            derivations.push(derivation);
            states.push(state);
        }
        let published = forest.publish(&derivations, governor)?;
        receipt.evidence = published.evidence;
        receipt.losses = published.losses;
        receipt.inferences = published.inferences;
        receipt.alternatives = published.alternatives;
        // `Outcome::Ambiguous` shares `receipt.alternatives`' published order.
        Ok(Outcome::Ambiguous(permute(&states, &published.order)))
    }
}

impl<S: SearchState> fmt::Debug for Search<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Search")
            .field("queued", &self.queue.len())
            .field("seen", &self.seen.len())
            .field("serial", &self.serial)
            .field("retained", &self.retained)
            .finish_non_exhaustive()
    }
}

impl<S> fmt::Debug for Accepted<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Accepted")
            .field("len", &self.candidates.len())
            .finish_non_exhaustive()
    }
}

impl<S: SearchState> fmt::Debug for Proposals<'_, S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Proposals")
            .field("parent", self.parent)
            .finish_non_exhaustive()
    }
}

impl<S: SearchState> Drop for Search<S> {
    fn drop(&mut self) {
        // Queue entries release their own leases; release the retained
        // deduplication keys' share here.
        let _ = self.meter.release(self.retained);
    }
}

#[cfg(test)]
mod tests {
    use super::{Acceptance, Change, Ledger, Proposals, Search, SearchState};
    use crate::contract::{Budget, Effort, Evidence, Governor, Outcome, Receipt, StopReason};
    use crate::{ScratchVec, contract};
    use core::cmp::Reverse;

    /// Synthetic search log: every accepted value, in call order.
    #[derive(Default)]
    struct Log {
        accepts: Vec<u32>,
        reverse_emission: bool,
    }

    /// A synthetic integer state. Successor graph (all at cost 1 unless
    /// noted): 0 -> {10, 20, 30 at cost 5, 40 at cost 5}; 10 -> 99; 20 -> 99; 1000 -> {1, 2, 3};
    /// 500 -> {5 at cost 10, 7}; 7 -> 5. Accepted: 1, 2, 3, 99.
    #[derive(Clone, Debug)]
    struct Int<T> {
        value: u32,
        tie: fn(u32) -> T,
    }

    fn change(rule: String, cost: u64) -> Change {
        Change {
            evidence: Evidence {
                rule,
                explanation: String::new(),
                span: None,
                destination: String::new(),
            },
            cost,
            original: None,
            guess: None,
        }
    }

    impl<T: Ord> SearchState for Int<T> {
        type Tie = T;
        type Context<'c> = Log;
        fn tie(&self) -> T {
            (self.tie)(self.value)
        }
        fn write_key(&self, key: &mut ScratchVec<u8>) -> Result<(), StopReason> {
            key.extend(self.value.to_be_bytes())
        }
        fn accept(&self, log: &mut Log, _: &mut Governor) -> Result<Acceptance, StopReason> {
            log.accepts.push(self.value);
            Ok(if matches!(self.value, 1..=3 | 99) {
                Acceptance::Accepted {
                    validation: log.accepts.len() - 1,
                }
            } else {
                Acceptance::Rejected
            })
        }
        fn expand(
            &self,
            log: &mut Log,
            proposals: &mut Proposals<'_, Self>,
            governor: &mut Governor,
        ) -> Result<(), StopReason> {
            let mut next: Vec<(u32, u64)> = match self.value {
                0 => vec![(10, 1), (20, 1), (30, 5), (40, 5)],
                10 | 20 => vec![(99, 1)],
                1000 => vec![(1, 1), (2, 1), (3, 1)],
                500 => vec![(5, 10), (7, 1)],
                7 => vec![(5, 1)],
                _ => Vec::new(),
            };
            if log.reverse_emission {
                next.reverse();
            }
            for (value, cost) in next {
                let state = Self {
                    value,
                    tie: self.tie,
                };
                let rule = format!("{}->{value}", self.value);
                proposals.commit(governor, state, change(rule, cost))?;
            }
            Ok(())
        }
    }

    struct Run<T> {
        outcome: Outcome<Int<T>>,
        receipt: Receipt,
        accepts: Vec<u32>,
        live_after_drop: u64,
    }

    fn search<T: Ord + Clone>(root: u32, tie: fn(u32) -> T, reverse_emission: bool) -> Run<T> {
        let budget = Budget::default();
        let mut governor = Governor::new(budget);
        let baseline = governor.scratch_meter().live();
        let mut receipt = Receipt::new(b"", "search.test", Effort::Standard, budget);
        let mut log = Log {
            accepts: Vec::new(),
            reverse_emission,
        };
        let outcome = {
            let mut search = Search::new(&governor);
            search
                .seed(&mut governor, Int { value: root, tie }, Ledger::default())
                .unwrap();
            let accepted = search.run(&mut log, &mut governor).unwrap();
            search
                .finish(accepted, &mut receipt, &mut governor)
                .unwrap()
        };
        // An ambiguous `finish` permanently retains its published clone
        // charges, so only non-ambiguous runs are compared to the baseline.
        let live_after_drop = governor.scratch_meter().live() - baseline;
        Run {
            outcome,
            receipt,
            accepts: log.accepts,
            live_after_drop,
        }
    }

    fn rules(receipt: &Receipt) -> Vec<String> {
        receipt.evidence.iter().map(|e| e.rule.clone()).collect()
    }

    #[test]
    fn total_order_independent_of_proposal_order() {
        let ascending = |value: u32| value;
        // A single winner reached through two equal-cost parents: the first
        // parent popped claims the shared child, and that is decided by the
        // tie key, never by emission order.
        for reverse in [false, true] {
            let run = search(0, ascending, reverse);
            assert_eq!(run.accepts, [0, 10, 20, 99], "reverse={reverse}");
            assert_eq!(rules(&run.receipt), ["0->10", "10->99"]);
            assert!(matches!(
                run.outcome,
                Outcome::Resolved(Int { value: 99, .. })
            ));
        }
        let descending = |value: u32| Reverse(value);
        for reverse in [false, true] {
            let run = search(0, descending, reverse);
            assert_eq!(run.accepts, [0, 20, 10, 99], "reverse={reverse}");
            assert_eq!(rules(&run.receipt), ["0->20", "20->99"]);
        }
        // Equal-cost accepted alternatives: acceptance order (and so each
        // alternative's validation index) and the published order agree.
        let forward = search(1000, ascending, false);
        let backward = search(1000, ascending, true);
        assert_eq!(forward.accepts, [1000, 1, 2, 3]);
        assert_eq!(forward.accepts, backward.accepts);
        let values = |outcome: &Outcome<Int<u32>>| match outcome {
            Outcome::Ambiguous(states) => states.iter().map(|s| s.value).collect::<Vec<_>>(),
            other => panic!("expected ambiguity, got {other:?}"),
        };
        assert_eq!(values(&forward.outcome), values(&backward.outcome));
        assert_eq!(forward.receipt.alternatives, backward.receipt.alternatives);
        assert_eq!(forward.receipt.alternatives.len(), 3);
    }

    #[test]
    fn stale_duplicate_is_not_revalidated() {
        // 5 is queued at cost 10, then again through 7 at cost 2; the stale
        // cost-10 entry must be skipped, not accepted or expanded again.
        let run = search(500, |value| value, false);
        assert_eq!(run.accepts, [500, 7, 5]);
        assert!(matches!(run.outcome, Outcome::Unresolved));
        assert_eq!(run.receipt.stopped, StopReason::NoCandidate);
    }

    #[test]
    fn scratch_returns_to_baseline_after_a_search() {
        // Resolved with candidates 30 and 40 left in the queue, and
        // unresolved after draining it: both release every enqueue charge.
        for root in [0, 500] {
            let run = search(root, |value| value, false);
            assert_eq!(run.live_after_drop, 0, "root={root}");
        }
        // Popping releases the entry share while the search is still alive:
        // only the retained key share and the key buffer remain.
        let mut governor = Governor::new(Budget::default());
        let mut search = Search::new(&governor);
        let mut log = Log::default();
        search
            .seed(
                &mut governor,
                Int {
                    value: 500,
                    tie: |v| v,
                },
                Ledger::default(),
            )
            .unwrap();
        let accepted = search.run(&mut log, &mut governor).unwrap();
        assert!(accepted.is_empty());
        let distinct_keys = 3; // 500, 5, 7
        let key_buffer = contract::saturating_size(4);
        assert_eq!(
            governor.scratch_meter().live(),
            distinct_keys * (4 + super::SEEN_OVERHEAD) + key_buffer
        );
        drop(search);
        assert_eq!(governor.scratch_meter().live(), 0);
    }
}
