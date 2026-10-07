// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! A deduplicated derivation forest for ambiguous receipts.
//!
//! Every governed operation that can return more than one competing answer
//! (`Outcome::Ambiguous`) must publish one [`AlternativeReceipt`] per answer
//! without losing track of evidence, losses, and inferences the competing
//! answers share. Building each alternative's ledger independently, as every
//! call site did before this module existed, either duplicates shared
//! evidence into every alternative or (worse) leaks it into the top-level,
//! selected-result ledgers where the contract forbids mutually exclusive
//! transformations from mixing (see [`crate::contract::Receipt::alternatives`]).
//!
//! A [`Forest`] fixes this structurally. Evidence, losses, inferences, and
//! validation references are *nodes*, interned once per distinct value
//! (content-addressed, like [`crate::digest::Digest`] but for structured
//! ledger entries rather than bytes). A [`Derivation`] is one candidate
//! answer: an output identity, a search cost, and the set of node ids it
//! depends on. [`Forest::publish`] partitions every node reachable from *any*
//! derivation into the nodes reachable from *every* derivation (the shared,
//! top-level ledgers) and the nodes specific to one derivation (that
//! derivation's [`AlternativeReceipt`]). The split is exact set intersection,
//! not a heuristic: two derivations share a node if and only if interning
//! produced the same [`NodeId`] for both, which happens if and only if the
//! underlying [`Evidence`]/[`Loss`]/[`Inference`] value compared equal.
//!
//! # Annotations
//!
//! The forest carries three independent read-only annotations, each a
//! standard semiring interpretation of "the set of derivations":
//!
//! - [`Forest::rank`] ranks derivations by ascending [`Derivation::cost`],
//!   the tropical (min-plus) semiring's [`⊕ = min`, `⊗ = +`] interpretation
//!   used for k-best selection: costs already accumulate by `+` as a
//!   derivation is built (see [`Derivation::push`]'s callers), and `rank`
//!   performs the `⊕ = min` reduction across derivations.
//! - [`Forest::multiplicity`] is the counting semiring: how many supplied
//!   derivations reach a given node, i.e. how many competing answers a piece
//!   of evidence supports.
//! - [`Forest::valid`] is the Boolean semiring: the conjunction, over every
//!   validation node a derivation reaches, of whether that validation
//!   passed — vacuously `true` for a derivation with no validation nodes.
//!
//! [`Forest::distinguishing`] answers "why is this ambiguous": the nodes
//! reachable from exactly one of two derivations (symmetric difference),
//! which is precisely the evidence that would let a caller tell them apart.
//!
//! # Resource contract
//!
//! Interning charges [`crate::contract::Resource::EvidenceEntries`] and
//! [`crate::contract::Resource::ScratchBytes`] exactly once per *distinct*
//! node, never once per derivation that reaches it: a forest with `N`
//! derivations sharing `E` distinct evidence nodes charges `E`, not `N * E`.
//! [`Forest::publish`] charges again, separately, for the clones it makes
//! while materializing the final ledgers (every node is cloned into exactly
//! one place: the shared ledgers, or exactly one alternative). Every charge
//! in this module happens before the allocation it bounds, using the same
//! [`crate::contract::ScratchLease`]/[`crate::contract::ScratchVec`]/
//! [`crate::contract::ScratchMap`] discipline as the rest of this crate,
//! including this module's own node table and dedup index (constructed by
//! [`Forest::new`] from a [`crate::contract::Governor`]).
//!
//! Because interning happens before anything is published, an exhausted
//! budget partway through building a forest's derivations never leaves a
//! caller with a partial alternative list: the caller simply never reaches
//! the point where it assigns `receipt.alternatives`, because the `?`
//! propagating the [`crate::contract::StopReason::BudgetExceeded`] unwinds
//! out of the whole build first. See the doctest below and
//! `crates/core/tests/provenance.rs` for both properties exercised
//! end-to-end.
//!
//! ```
//! use purrassist_core::{
//!     Budget, Governor,
//!     contract::{Evidence, Resource},
//!     digest::Digest,
//!     provenance::{Derivation, Forest},
//! };
//!
//! let mut governor = Governor::new(Budget::default());
//! let mut forest = Forest::new(&governor);
//!
//! let shared = Evidence {
//!     rule: "example.shared".into(),
//!     explanation: "both candidates matched the same rule".into(),
//!     span: None,
//!     destination: "/value".into(),
//! };
//! let mut first = Derivation::new(Digest::of(b"1"), 0);
//! first.push(forest.evidence(&mut governor, shared.clone())?);
//! first.push(forest.evidence(
//!     &mut governor,
//!     Evidence { explanation: "picked 1".into(), ..shared.clone() },
//! )?);
//! let mut second = Derivation::new(Digest::of(b"2"), 0);
//! second.push(forest.evidence(&mut governor, shared.clone())?);
//! second.push(forest.evidence(
//!     &mut governor,
//!     Evidence { explanation: "picked 2".into(), ..shared.clone() },
//! )?);
//!
//! // The shared node was interned once: two derivations, one evidence entry.
//! assert_eq!(governor.usage().evidence_entries, 3);
//!
//! let distinguishing = forest.distinguishing(&first, &second);
//! assert_eq!(distinguishing.len(), 2); // each derivation's own, distinguishing evidence.
//!
//! let published = forest.publish(&[first, second], &mut governor)?;
//! assert_eq!(published.evidence, vec![shared]); // promoted to the shared ledger.
//! assert_eq!(published.alternatives.len(), 2);
//! for alternative in &published.alternatives {
//!     assert_eq!(alternative.evidence.len(), 1); // only the distinguishing entry remains.
//! }
//! # Ok::<(), purrassist_core::StopReason>(())
//! ```

use crate::contract::saturating_size as size;
use crate::contract::{
    AlternativeReceipt, Evidence, Governor, Inference, Loss, Resource, ScratchMap, ScratchVec,
    StopReason,
};
use crate::digest::Digest;
use std::collections::BTreeSet;

/// Stable identity of one deduplicated node within one [`Forest`].
///
/// Ids are assigned in interning order and are only comparable within the
/// [`Forest`] that issued them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeId(u32);

/// One deduplicated unit of provenance.
///
/// Evidence, a loss, an inference, or a reference to a validation already
/// recorded in the enclosing receipt's validation log (identified by index,
/// with its passing status carried alongside for the Boolean-validation
/// annotation, see [`Forest::valid`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProvenanceNode {
    /// One recorded transformation or selection decision.
    Evidence(Evidence),
    /// One discarded or irreversibly changed piece of information.
    Loss(Loss),
    /// One derived or invented output value.
    Inference(Inference),
    /// An index into the enclosing receipt's validation log, and whether
    /// that validation passed.
    Validation(usize, bool),
}

fn evidence_bytes(evidence: &Evidence) -> u64 {
    size(evidence.rule.len())
        .saturating_add(size(evidence.explanation.len()))
        .saturating_add(size(evidence.destination.len()))
        .saturating_add(32)
}
fn node_bytes(node: &ProvenanceNode) -> u64 {
    match node {
        ProvenanceNode::Evidence(evidence) => evidence_bytes(evidence),
        ProvenanceNode::Loss(loss) => {
            evidence_bytes(&loss.evidence).saturating_add(size(loss.original.len()))
        }
        ProvenanceNode::Inference(inference) => {
            evidence_bytes(&inference.evidence).saturating_add(size(inference.value.len()))
        }
        ProvenanceNode::Validation(..) => 16,
    }
}
/// A conservative bound on interning `node`: its own content, plus a
/// generous multiplier for the `Debug`-formatted dedup key and the node's
/// place in the forest's own storage, charged once, before either exists.
fn intern_reservation(node: &ProvenanceNode) -> u64 {
    node_bytes(node).saturating_mul(6).saturating_add(512)
}
/// A conservative bound on cloning `node` once into a published ledger.
fn clone_reservation(node: &ProvenanceNode) -> u64 {
    node_bytes(node).saturating_mul(2).saturating_add(128)
}

/// One candidate answer: an output identity, a search cost comparable only
/// within the same operation, and the (deduplicated, insertion-ordered) set
/// of forest nodes this answer depends on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Derivation {
    output: Digest,
    cost: u64,
    order: Vec<NodeId>,
}
impl Derivation {
    /// Start a derivation for one candidate answer, identified by the digest
    /// of its wire encoding (see `crate::wire`) and its search cost.
    #[must_use]
    pub const fn new(output: Digest, cost: u64) -> Self {
        Self {
            output,
            cost,
            order: Vec::new(),
        }
    }
    /// Record that this derivation depends on `node`, preserving first-seen
    /// order and silently ignoring a duplicate id.
    pub fn push(&mut self, node: NodeId) {
        if !self.order.contains(&node) {
            self.order.push(node);
        }
    }
    /// Whether this derivation depends on `node`.
    #[must_use]
    pub fn contains(&self, node: NodeId) -> bool {
        self.order.contains(&node)
    }
    /// This derivation's output identity.
    #[must_use]
    pub const fn output(&self) -> Digest {
        self.output
    }
    /// This derivation's search cost.
    #[must_use]
    pub const fn cost(&self) -> u64 {
        self.cost
    }
    /// The forest nodes this derivation depends on, in first-seen order.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, contract::Evidence, digest::Digest};
    /// use purrassist_core::provenance::{Derivation, Forest};
    /// let mut governor = Governor::new(Budget::default());
    /// let mut forest = Forest::new(&governor);
    /// let evidence = Evidence {
    ///     rule: "demo".into(),
    ///     explanation: String::new(),
    ///     span: None,
    ///     destination: String::new(),
    /// };
    /// let node = forest.evidence(&mut governor, evidence)?;
    /// let mut derivation = Derivation::new(Digest::of(b"x"), 1);
    /// derivation.push(node);
    /// derivation.push(node);
    /// assert_eq!(derivation.nodes(), [node]);
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    #[must_use]
    pub fn nodes(&self) -> &[NodeId] {
        &self.order
    }
}

/// The result of [`Forest::publish`]: shared ledgers promoted out of every
/// derivation, and the per-derivation ledgers that remain.
#[derive(Clone, Debug, Default)]
pub struct Published {
    /// Evidence common to every supplied derivation.
    pub evidence: Vec<Evidence>,
    /// Losses common to every supplied derivation.
    pub losses: Vec<Loss>,
    /// Inferences common to every supplied derivation.
    pub inferences: Vec<Inference>,
    /// One ledger per supplied derivation, already reordered to ascending
    /// [`Forest::rank`] (ties broken by output digest). Empty when fewer
    /// than two derivations were published, matching `Receipt::alternatives`'
    /// "empty for operations without alternative ledgers" contract.
    pub alternatives: Vec<AlternativeReceipt>,
    /// The original `derivations` index now published at each position of
    /// [`Published::alternatives`]: `order[k]` is the index into the
    /// `derivations` slice (and, by construction, into the caller's own
    /// parallel candidate-output slice) that now sits at position `k`.
    /// Every caller that builds an `Outcome::Ambiguous` value from the same
    /// `derivations` **must** reorder its own candidate vector by this same
    /// permutation (see [`permute`]) before constructing it, so
    /// `Outcome::Ambiguous`, `Receipt::alternatives`, and every wire
    /// encoding of both share exactly one order — never reorder only at a
    /// wire boundary. Identity (`0..derivations.len()`) when fewer than two
    /// derivations were published (nothing to reorder).
    pub order: Vec<usize>,
}

/// Reorder `values` (indexed by original derivation index) to [`Published::order`].
///
/// `permute(values, &published.order)[k] == values[published.order[k]]`.
/// See [`Published::order`] for why every `Outcome::Ambiguous` call site
/// must apply this to its own candidate vector.
#[must_use]
pub fn permute<T: Clone>(values: &[T], order: &[usize]) -> Vec<T> {
    order.iter().map(|&index| values[index].clone()).collect()
}

/// A deduplicated forest of provenance nodes shared by every derivation of
/// one governed decision. See the module documentation for the model and
/// resource contract.
#[derive(Debug)]
pub struct Forest {
    nodes: ScratchVec<ProvenanceNode>,
    index: ScratchMap<String, NodeId>,
}
impl Forest {
    /// An empty forest charging against `governor`'s scratch meter.
    #[must_use]
    pub fn new(governor: &Governor) -> Self {
        Self {
            nodes: ScratchVec::new(governor),
            index: ScratchMap::new(governor),
        }
    }
    /// Intern one evidence entry, returning its (possibly pre-existing) id.
    ///
    /// # Errors
    /// Returns `BudgetExceeded` before allocating, and before charging
    /// `EvidenceEntries`, if `evidence` was not already interned.
    pub fn evidence(
        &mut self,
        governor: &mut Governor,
        evidence: Evidence,
    ) -> Result<NodeId, StopReason> {
        self.intern(governor, ProvenanceNode::Evidence(evidence))
    }
    /// Intern one loss entry, returning its (possibly pre-existing) id.
    ///
    /// # Errors
    /// Returns `BudgetExceeded` before allocating, and before charging
    /// `EvidenceEntries`, if `loss` was not already interned.
    pub fn loss(&mut self, governor: &mut Governor, loss: Loss) -> Result<NodeId, StopReason> {
        self.intern(governor, ProvenanceNode::Loss(loss))
    }
    /// Intern one inference entry, returning its (possibly pre-existing) id.
    ///
    /// # Errors
    /// Returns `BudgetExceeded` before allocating, and before charging
    /// `EvidenceEntries`, if `inference` was not already interned.
    pub fn inference(
        &mut self,
        governor: &mut Governor,
        inference: Inference,
    ) -> Result<NodeId, StopReason> {
        self.intern(governor, ProvenanceNode::Inference(inference))
    }
    /// Intern a reference to validation log entry `index`, whose outcome was
    /// `passed`, returning its (possibly pre-existing) id.
    ///
    /// # Errors
    /// Returns `BudgetExceeded` before allocating, and before charging
    /// `EvidenceEntries`, if this exact `(index, passed)` was not already
    /// interned.
    pub fn validation(
        &mut self,
        governor: &mut Governor,
        index: usize,
        passed: bool,
    ) -> Result<NodeId, StopReason> {
        self.intern(governor, ProvenanceNode::Validation(index, passed))
    }
    fn intern(
        &mut self,
        governor: &mut Governor,
        node: ProvenanceNode,
    ) -> Result<NodeId, StopReason> {
        let lease = governor.lease_scratch(intern_reservation(&node))?;
        let key = format!("{node:?}");
        if let Some(&id) = self.index.get(&key) {
            return Ok(id); // `lease` drops here, releasing the transient key's bytes.
        }
        governor.charge(Resource::EvidenceEntries, 1)?;
        lease.retain(); // Now backing this node's permanently retained storage.
        let id = NodeId(
            u32::try_from(self.nodes.len())
                .map_err(|_| StopReason::BudgetExceeded(Resource::EvidenceEntries))?,
        );
        self.nodes.push(node)?;
        self.index.insert(key, id)?;
        Ok(id)
    }
    /// Every interned node, in interning order ([`NodeId`] order).
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, contract::Evidence, provenance::{Forest, ProvenanceNode}};
    /// let mut governor = Governor::new(Budget::default());
    /// let mut forest = Forest::new(&governor);
    /// assert!(forest.nodes().is_empty());
    /// let evidence = Evidence {
    ///     rule: "demo".into(),
    ///     explanation: String::new(),
    ///     span: None,
    ///     destination: String::new(),
    /// };
    /// forest.evidence(&mut governor, evidence.clone())?;
    /// assert_eq!(forest.nodes(), [ProvenanceNode::Evidence(evidence)]);
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    #[must_use]
    pub fn nodes(&self) -> &[ProvenanceNode] {
        self.nodes.as_slice()
    }
    /// Borrow the content of one previously interned node.
    #[must_use]
    pub fn node(&self, id: NodeId) -> &ProvenanceNode {
        &self.nodes.as_slice()[id.0 as usize]
    }
    /// Tropical (min-plus) semiring rank: derivation indices in ascending
    /// cost order (`⊕ = min` over derivations, `⊗ = +` already folded into
    /// each derivation's accumulated [`Derivation::cost`]), the standard
    /// k-best selection order. Ties break on ascending [`Derivation::output`]
    /// (a total order, since [`Digest`] is [`Ord`]), so the result is
    /// deterministic independent of input order; [`Forest::publish`] applies
    /// this order and reports it as [`Published::order`], so every ambiguous
    /// outcome, receipt, and wire form lists alternatives in it.
    #[must_use]
    pub fn rank(derivations: &[Derivation]) -> Vec<usize> {
        let mut order: Vec<usize> = (0..derivations.len()).collect();
        order.sort_by(|&a, &b| {
            derivations[a]
                .cost
                .cmp(&derivations[b].cost)
                .then_with(|| derivations[a].output.cmp(&derivations[b].output))
        });
        order
    }
    /// Counting semiring: how many of `derivations` depend on `node`.
    #[must_use]
    pub fn multiplicity(derivations: &[Derivation], node: NodeId) -> usize {
        derivations
            .iter()
            .filter(|derivation| derivation.contains(node))
            .count()
    }
    /// Boolean semiring: whether every validation node `derivation` depends
    /// on passed (vacuously `true` if it depends on none).
    #[must_use]
    pub fn valid(&self, derivation: &Derivation) -> bool {
        derivation
            .order
            .iter()
            .all(|&id| !matches!(self.node(id), ProvenanceNode::Validation(_, passed) if !passed))
    }
    /// The nodes reachable from exactly one of `a` or `b` (symmetric
    /// difference), in ascending [`NodeId`] order: the evidence that would
    /// let a caller tell these two derivations apart.
    #[must_use]
    pub fn distinguishing<'a>(&'a self, a: &Derivation, b: &Derivation) -> Vec<&'a ProvenanceNode> {
        let left: BTreeSet<NodeId> = a.order.iter().copied().collect();
        let right: BTreeSet<NodeId> = b.order.iter().copied().collect();
        left.symmetric_difference(&right)
            .copied()
            .map(|id| self.node(id))
            .collect()
    }
    fn charge_clone(&self, governor: &mut Governor, id: NodeId) -> Result<(), StopReason> {
        governor.charge(Resource::ScratchBytes, clone_reservation(self.node(id)))
    }
    /// Partition every node reachable from `derivations` into the nodes
    /// shared by *all* of them (promoted to [`Published::evidence`]/
    /// [`Published::losses`]/[`Published::inferences`]) and the nodes
    /// specific to exactly one derivation (that derivation's
    /// [`AlternativeReceipt`] in [`Published::alternatives`]). Validation
    /// nodes are never promoted (there is no top-level validation ledger to
    /// promote them to); every derivation keeps its own complete,
    /// deduplicated, order-preserving list of validation indices.
    ///
    /// Fewer than two derivations publish everything as shared, with no
    /// alternatives, matching `Receipt::alternatives`' contract for
    /// operations with a single answer.
    ///
    /// # Errors
    /// Returns `BudgetExceeded` before cloning a node into its published
    /// location; nothing is returned (no partial [`Published`] value exists)
    /// once this happens.
    pub fn publish(
        &self,
        derivations: &[Derivation],
        governor: &mut Governor,
    ) -> Result<Published, StopReason> {
        if derivations.len() < 2 {
            let mut published = Published {
                order: (0..derivations.len()).collect(),
                ..Published::default()
            };
            if let Some(derivation) = derivations.first() {
                for &id in &derivation.order {
                    self.charge_clone(governor, id)?;
                    match self.node(id) {
                        ProvenanceNode::Evidence(evidence) => {
                            published.evidence.push(evidence.clone());
                        }
                        ProvenanceNode::Loss(loss) => published.losses.push(loss.clone()),
                        ProvenanceNode::Inference(inference) => {
                            published.inferences.push(inference.clone());
                        }
                        ProvenanceNode::Validation(..) => {}
                    }
                }
            }
            return Ok(published);
        }
        // A node is shared exactly when every derivation reaches it: the
        // counting-semiring annotation `multiplicity(derivations, id) ==
        // derivations.len()`, computed here through the public
        // `Self::multiplicity` rather than a private set intersection, so
        // this promotion decision and that annotation can never disagree.
        let mut candidates: BTreeSet<NodeId> = BTreeSet::new();
        for derivation in derivations {
            for &id in &derivation.order {
                if !matches!(self.node(id), ProvenanceNode::Validation(..)) {
                    candidates.insert(id);
                }
            }
        }
        let mut shared: BTreeSet<NodeId> = BTreeSet::new();
        for id in candidates {
            if Self::multiplicity(derivations, id) == derivations.len() {
                shared.insert(id);
            }
        }
        let mut evidence = Vec::new();
        let mut losses = Vec::new();
        let mut inferences = Vec::new();
        for &id in &shared {
            self.charge_clone(governor, id)?;
            match self.node(id) {
                ProvenanceNode::Evidence(entry) => evidence.push(entry.clone()),
                ProvenanceNode::Loss(entry) => losses.push(entry.clone()),
                ProvenanceNode::Inference(entry) => inferences.push(entry.clone()),
                ProvenanceNode::Validation(..) => unreachable!("filtered above"),
            }
        }
        // `ranked` is `Self::rank`'s ascending-cost, digest-tie-broken
        // derivation order; `alternatives` (and the `order` this function
        // returns) are built in exactly this order, giving `Self::rank` a
        // production consumer beyond this crate's own tests and ensuring
        // every caller's own candidate vector, once permuted the same way
        // (see [`Published::order`]), agrees with it.
        let ranked = Self::rank(derivations);
        let mut alternatives = Vec::with_capacity(derivations.len());
        for &derivation_index in &ranked {
            let derivation = &derivations[derivation_index];
            let mut own_evidence = Vec::new();
            let mut own_losses = Vec::new();
            let mut own_inferences = Vec::new();
            let mut validations = Vec::new();
            for &id in &derivation.order {
                match self.node(id) {
                    ProvenanceNode::Validation(index, _) => {
                        if !validations.contains(index) {
                            validations.push(*index);
                        }
                    }
                    _ if shared.contains(&id) => {}
                    ProvenanceNode::Evidence(entry) => {
                        self.charge_clone(governor, id)?;
                        own_evidence.push(entry.clone());
                    }
                    ProvenanceNode::Loss(entry) => {
                        self.charge_clone(governor, id)?;
                        own_losses.push(entry.clone());
                    }
                    ProvenanceNode::Inference(entry) => {
                        self.charge_clone(governor, id)?;
                        own_inferences.push(entry.clone());
                    }
                }
            }
            alternatives.push(AlternativeReceipt {
                output: derivation.output,
                cost: derivation.cost,
                evidence: own_evidence,
                losses: own_losses,
                inferences: own_inferences,
                validations,
                valid: self.valid(derivation),
            });
        }
        Ok(Published {
            evidence,
            losses,
            inferences,
            alternatives,
            order: ranked,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Derivation, Forest};
    use crate::contract::{Evidence, Inference, InferenceKind, Loss, Resource};
    use crate::digest::Digest;
    use crate::{Budget, Governor, StopReason};

    fn evidence(explanation: &str) -> Evidence {
        Evidence {
            rule: "test.rule".into(),
            explanation: explanation.into(),
            span: None,
            destination: "/x".into(),
        }
    }

    #[test]
    fn identical_content_dedups_to_one_node_and_one_charge() {
        let mut governor = Governor::new(Budget::default());
        let mut forest = Forest::new(&governor);
        let a = forest.evidence(&mut governor, evidence("same")).unwrap();
        let b = forest.evidence(&mut governor, evidence("same")).unwrap();
        assert_eq!(a, b);
        assert_eq!(governor.usage().evidence_entries, 1);
        let c = forest
            .evidence(&mut governor, evidence("different"))
            .unwrap();
        assert_ne!(a, c);
        assert_eq!(governor.usage().evidence_entries, 2);
    }

    #[test]
    fn n_alternatives_sharing_e_evidence_charge_e_not_n_times_e() {
        let mut governor = Governor::new(Budget::default());
        let mut forest = Forest::new(&governor);
        let shared = evidence("shared across every candidate");
        let mut derivations = Vec::new();
        for index in 0..5 {
            let mut derivation = Derivation::new(Digest::of(&[index]), 0);
            derivation.push(forest.evidence(&mut governor, shared.clone()).unwrap());
            derivation.push(
                forest
                    .evidence(&mut governor, evidence(&format!("distinct {index}")))
                    .unwrap(),
            );
            derivations.push(derivation);
        }
        // 1 shared node + 5 distinct nodes = 6 distinct nodes total, not 10.
        assert_eq!(governor.usage().evidence_entries, 6);
        let published = forest.publish(&derivations, &mut governor).unwrap();
        assert_eq!(published.evidence, vec![shared]);
        assert_eq!(published.alternatives.len(), 5);
        for alternative in &published.alternatives {
            assert_eq!(alternative.evidence.len(), 1);
        }
    }

    #[test]
    fn exhaustion_mid_build_publishes_nothing() {
        let mut governor = Governor::new(Budget {
            evidence_entries: 1,
            ..Budget::default()
        });
        let mut forest = Forest::new(&governor);
        let mut derivations = Vec::new();
        let mut first = Derivation::new(Digest::of(b"1"), 0);
        first.push(forest.evidence(&mut governor, evidence("first")).unwrap());
        derivations.push(first);
        let second = Derivation::new(Digest::of(b"2"), 0);
        let failure = forest.evidence(&mut governor, evidence("second"));
        assert_eq!(
            failure,
            Err(StopReason::BudgetExceeded(Resource::EvidenceEntries))
        );
        // The caller never finishes building `second`, so it never calls
        // `publish`: nothing is published, matching the "no alternatives
        // published" contract for exhaustion mid-build.
        derivations.push(second);
        assert_eq!(derivations.len(), 2);
    }

    #[test]
    fn distinguishing_and_annotations() {
        let mut governor = Governor::new(Budget::default());
        let mut forest = Forest::new(&governor);
        let shared = forest.evidence(&mut governor, evidence("shared")).unwrap();
        let mut a = Derivation::new(Digest::of(b"a"), 5);
        a.push(shared);
        let only_a = forest.evidence(&mut governor, evidence("only a")).unwrap();
        a.push(only_a);
        let mut b = Derivation::new(Digest::of(b"b"), 2);
        b.push(shared);
        let only_b = forest.evidence(&mut governor, evidence("only b")).unwrap();
        b.push(only_b);

        let distinguishing = forest.distinguishing(&a, &b);
        assert_eq!(distinguishing.len(), 2);

        assert_eq!(Forest::rank(&[a.clone(), b.clone()]), vec![1, 0]); // b (cost 2) before a (cost 5).
        assert_eq!(Forest::multiplicity(&[a.clone(), b.clone()], shared), 2);
        assert_eq!(Forest::multiplicity(&[a.clone(), b.clone()], only_a), 1);
        assert!(forest.valid(&a));
    }

    #[test]
    fn boolean_validation_annotation_is_a_conjunction() {
        let mut governor = Governor::new(Budget::default());
        let mut forest = Forest::new(&governor);
        let mut derivation = Derivation::new(Digest::of(b"x"), 0);
        derivation.push(forest.validation(&mut governor, 0, true).unwrap());
        assert!(forest.valid(&derivation));
        derivation.push(forest.validation(&mut governor, 1, false).unwrap());
        assert!(!forest.valid(&derivation));
    }

    #[test]
    fn published_validations_are_never_split_by_sharing() {
        let mut governor = Governor::new(Budget::default());
        let mut forest = Forest::new(&governor);
        let mut a = Derivation::new(Digest::of(b"a"), 0);
        a.push(forest.validation(&mut governor, 0, true).unwrap());
        a.push(forest.evidence(&mut governor, evidence("a")).unwrap());
        let mut b = Derivation::new(Digest::of(b"b"), 0);
        b.push(forest.validation(&mut governor, 1, true).unwrap());
        b.push(forest.evidence(&mut governor, evidence("b")).unwrap());
        let published = forest.publish(&[a, b], &mut governor).unwrap();
        // Both derivations cost 0 (a tie), so `publish` orders them by
        // ascending output digest, not input order; assert the split by
        // content rather than a hard-coded position.
        assert_eq!(published.alternatives.len(), 2);
        assert!(
            published
                .alternatives
                .iter()
                .any(|entry| entry.validations == vec![0])
        );
        assert!(
            published
                .alternatives
                .iter()
                .any(|entry| entry.validations == vec![1])
        );
    }

    #[test]
    fn losses_and_inferences_intern_and_publish_like_evidence() {
        let mut governor = Governor::new(Budget::default());
        let mut forest = Forest::new(&governor);
        let loss = Loss {
            evidence: evidence("dropped"),
            original: "original text".into(),
        };
        let inference = Inference {
            evidence: evidence("guessed"),
            kind: InferenceKind::Guess,
            value: "42".into(),
        };
        let mut only = Derivation::new(Digest::of(b"only"), 0);
        only.push(forest.loss(&mut governor, loss.clone()).unwrap());
        only.push(forest.inference(&mut governor, inference.clone()).unwrap());
        let published = forest.publish(&[only], &mut governor).unwrap();
        assert_eq!(published.losses, vec![loss]);
        assert_eq!(published.inferences, vec![inference]);
        assert_eq!(published.alternatives, Vec::new());
    }
}
