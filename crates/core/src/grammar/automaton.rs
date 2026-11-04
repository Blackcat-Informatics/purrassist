// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Compiled byte tables: labelled literal tries and dense Aho–Corasick
//! matchers.
//!
//! A [`Trie`] recognizes a small set of labelled spellings one byte at a
//! time (closer variants, confusable spellings, stray closers). A
//! [`Matcher`] finds the first occurrence of any literal of a delimiter set
//! in a byte stream in O(1) per byte; it drives `until(...)` bodies, codec
//! slots, and reasoning markers. Sparse trie nodes/edges retain fallible
//! scratch ownership. Dense matcher construction and final tables use the same
//! fallible ownership and explicit resource admission.
// `unreachable_pub` and `redundant_pub_crate` disagree on the right
// visibility for items of a crate-private module
// (https://github.com/rust-lang/rust-clippy/issues/6188); these items are
// shared across the grammar and tool-call modules, so `pub(crate)` (matching
// `unreachable_pub`) wins, as in `pattern::derive`.
#![allow(clippy::redundant_pub_crate)]
use super::cost::K_COMPILE;
use crate::contract::saturating_size as size;
use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchLease, ScratchVec, StopReason, invalid_input,
    push_scratch,
};

/// Byte-identical node id of a trie or matcher root.
pub(crate) const ROOT: u32 = 0;

#[derive(Debug, PartialEq, Eq)]
struct TrieNode {
    children: ScratchVec<(u8, u32)>,
    accept: Option<u32>,
}
impl ScratchCharge for TrieNode {
    const CHARGE: usize = 48;
}
const _: () = assert!(core::mem::size_of::<TrieNode>() <= TrieNode::CHARGE);

impl TrieNode {
    fn new(governor: &Governor) -> Self {
        Self {
            children: ScratchVec::new(governor),
            accept: None,
        }
    }
}

/// A sparse trie of labelled byte spellings, with release-owned nodes and edges.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Trie {
    nodes: ScratchVec<TrieNode>,
}

/// The actual owner and iterator descriptors used during trie construction.
struct TrieBuild<I> {
    nodes: ScratchVec<TrieNode>,
    source: I,
}

struct TrieCursor<'a> {
    bytes: &'a [u8],
    label: u32,
    node: usize,
    at: usize,
    index: usize,
    next: usize,
    id: u32,
    comparisons: u64,
    byte: u8,
}

impl TrieCursor<'_> {
    const fn new() -> Self {
        Self {
            bytes: &[],
            label: 0,
            node: 0,
            at: 0,
            index: 0,
            next: 0,
            id: 0,
            comparisons: 0,
            byte: 0,
        }
    }
}

impl Trie {
    /// A trie holding borrowed `spellings`; the first label inserted wins.
    /// Scans, comparisons and insertion shifts are admitted before processing.
    pub(crate) fn build<'a>(
        spellings: impl IntoIterator<Item = (&'a [u8], u32)>,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        // Flat storage and iteration require one frame, independent of byte length.
        governor.charge(Resource::Depth, 1)?;
        governor.charge(Resource::Work, K_COMPILE)?;
        let mut build = TrieBuild {
            nodes: ScratchVec::new(governor),
            source: spellings.into_iter(),
        };
        let mut cursor = TrieCursor::new();
        build.nodes.push(TrieNode::new(governor))?;
        while let Some((bytes, label)) = build.source.next() {
            governor.charge(Resource::Work, K_COMPILE)?;
            cursor.bytes = bytes;
            cursor.label = label;
            cursor.node = ROOT as usize;
            cursor.at = 0;
            while cursor.at < cursor.bytes.len() {
                cursor.byte = cursor.bytes[cursor.at];
                let children = &build.nodes[cursor.node].children;
                cursor.comparisons = u64::from(children.len().bit_width()) + 1;
                governor.charge(Resource::Work, cursor.comparisons * K_COMPILE)?;
                cursor.node = match children.binary_search_by_key(&cursor.byte, |(key, _)| *key) {
                    Ok(index) => children[index].1 as usize,
                    Err(index) => {
                        cursor.index = index;
                        cursor.next = build.nodes.len();
                        cursor.id = u32::try_from(cursor.next)
                            .map_err(|_| governor.stop_resource(Resource::ScratchBytes))?;
                        governor.charge(
                            Resource::Work,
                            size(children.len() - cursor.index + 1) * K_COMPILE,
                        )?;
                        // Fallible growth precedes allocation-free sorted insertion.
                        build.nodes.push(TrieNode::new(governor))?;
                        let children = &mut build.nodes.as_mut_slice()[cursor.node].children;
                        children.push((cursor.byte, cursor.id))?;
                        children.as_mut_slice()[cursor.index..].rotate_right(1);
                        cursor.next
                    }
                };
                cursor.at += 1;
            }
            build.nodes.as_mut_slice()[cursor.node]
                .accept
                .get_or_insert(cursor.label);
        }
        governor.check()?;
        Ok(Self { nodes: build.nodes })
    }

    /// The child of `node` on `byte`.
    pub(crate) fn step(&self, node: u32, byte: u8) -> Option<u32> {
        let children = &self.nodes[node as usize].children;
        children
            .binary_search_by_key(&byte, |(key, _)| *key)
            .ok()
            .map(|index| children[index].1)
    }

    /// The label of the spelling ending at `node`, if one does.
    pub(crate) fn accept(&self, node: u32) -> Option<u32> {
        self.nodes[node as usize].accept
    }

    /// Whether some longer spelling continues past `node`.
    pub(crate) fn has_children(&self, node: u32) -> bool {
        !self.nodes[node as usize].children.is_empty()
    }

    /// Whether the trie holds no spelling.
    pub(crate) const fn is_empty(&self) -> bool {
        self.nodes.len() == 1
    }

    /// Whether `bytes` could be (a prefix of) a spelling, or a complete
    /// spelling followed by more bytes.
    pub(crate) fn admits(&self, bytes: &[u8]) -> bool {
        let mut node = ROOT;
        for &byte in bytes {
            if self.accept(node).is_some() && node != ROOT {
                return true;
            }
            match self.step(node, byte) {
                Some(next) => node = next,
                None => return false,
            }
        }
        true
    }

    /// The first byte of every spelling.
    pub(crate) fn first_bytes(&self) -> impl Iterator<Item = u8> + '_ {
        self.nodes[ROOT as usize]
            .children
            .iter()
            .map(|(byte, _)| *byte)
    }
}

/// A dense Aho–Corasick automaton over a set of non-empty byte literals.
///
/// By default, [`Matcher::found`] reports the longest literal that ends at
/// each byte. [`Matcher::build_first`] instead selects the first input literal
/// among those ending together. Both select the earliest-ending occurrence;
/// [`Matcher::depth`] the length of the longest suffix represented by a trie
/// prefix, including completed leaves. Marker holdback uses the separately
/// prepared proper-continuation depths from [`Matcher::continuation_depths`].
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Matcher {
    delta: ScratchVec<u32>,
    depth: ScratchVec<u32>,
    found: ScratchVec<Option<(u32, u32)>>,
    longest: usize,
}

/// Concrete borrowed input keeps the nested trie iterator descriptor explicit.
struct IndexedLiterals<'a, B> {
    literals: &'a [B],
    next: usize,
}

impl<'a, B: AsRef<[u8]>> Iterator for IndexedLiterals<'a, B> {
    type Item = (&'a [u8], u32);

    fn next(&mut self) -> Option<Self::Item> {
        let literal = self.literals.get(self.next)?;
        let label = u32::try_from(self.next).expect("admitted literal index");
        self.next += 1;
        Some((literal.as_ref(), label))
    }
}

struct MatcherBuild {
    goto: Trie,
    delta: ScratchVec<u32>,
    depth: ScratchVec<u32>,
    fail: ScratchVec<u32>,
    found: ScratchVec<Option<(u32, u32)>>,
    queue: ScratchVec<u32>,
    count: usize,
    cells: usize,
    cursor: usize,
}

struct MatcherCursor<'a> {
    longest: usize,
    literal: usize,
    bytes: &'a [u8],
    state: usize,
    fallback: usize,
    edge: usize,
    byte: u8,
    slot: usize,
    fallback_state: u32,
    position: usize,
    child: u32,
    comparisons: u64,
    next_depth: u32,
    current: Option<(u32, u32)>,
    inherited: Option<(u32, u32)>,
}

impl MatcherCursor<'_> {
    const fn new() -> Self {
        Self {
            longest: 0,
            literal: 0,
            bytes: &[],
            state: 0,
            fallback: 0,
            edge: 0,
            byte: 0,
            slot: 0,
            fallback_state: 0,
            position: 0,
            child: 0,
            comparisons: 0,
            next_depth: 0,
            current: None,
            inherited: None,
        }
    }
}

impl MatcherBuild {
    fn allocate(goto: Trie, governor: &mut Governor) -> Result<Self, StopReason> {
        let mut build = Self {
            goto,
            delta: ScratchVec::new(governor),
            depth: ScratchVec::new(governor),
            fail: ScratchVec::new(governor),
            found: ScratchVec::new(governor),
            queue: ScratchVec::new(governor),
            count: 0,
            cells: 0,
            cursor: 0,
        };
        build.count = build.goto.nodes.len();
        build.cells = build
            .count
            .checked_mul(256)
            .ok_or_else(|| governor.stop_resource(Resource::ScratchBytes))?;
        // Admit zero initialization of all final and temporary table elements.
        let work = size(build.cells)
            .checked_add(
                size(build.count)
                    .checked_mul(3)
                    .ok_or_else(|| governor.stop_resource(Resource::Work))?,
            )
            .ok_or_else(|| governor.stop_resource(Resource::Work))?;
        governor.charge(Resource::Work, work)?;
        build
            .delta
            .extend(core::iter::repeat_n(0_u32, build.cells))?;
        build
            .depth
            .extend(core::iter::repeat_n(0_u32, build.count))?;
        build
            .fail
            .extend(core::iter::repeat_n(0_u32, build.count))?;
        build
            .found
            .extend(core::iter::repeat_n(None, build.count))?;
        Ok(build)
    }

    fn prefixes(
        &mut self,
        cursor: &mut MatcherCursor<'_>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        // Trie insertion always assigns a parent before its children.
        cursor.state = 0;
        while cursor.state < self.count {
            let node = &self.goto.nodes[cursor.state];
            governor.charge(Resource::Work, size(node.children.len()).saturating_add(1))?;
            cursor.edge = 0;
            while cursor.edge < node.children.len() {
                cursor.child = node.children[cursor.edge].1;
                cursor.next_depth = self.depth[cursor.state]
                    .checked_add(1)
                    .ok_or_else(|| governor.stop_resource(Resource::ScratchBytes))?;
                self.depth.as_mut_slice()[cursor.child as usize] = cursor.next_depth;
                cursor.edge += 1;
            }
            self.found.as_mut_slice()[cursor.state] =
                node.accept.map(|label| (label, self.depth[cursor.state]));
            cursor.state += 1;
        }
        governor.charge(Resource::Work, size(self.goto.nodes[0].children.len()))?;
        cursor.edge = 0;
        while cursor.edge < self.goto.nodes[0].children.len() {
            cursor.byte = self.goto.nodes[0].children[cursor.edge].0;
            cursor.child = self.goto.nodes[0].children[cursor.edge].1;
            self.delta.as_mut_slice()[usize::from(cursor.byte)] = cursor.child;
            self.queue.push(cursor.child)?;
            cursor.edge += 1;
        }
        Ok(())
    }

    fn transitions<const FIRST: bool>(
        &mut self,
        cursor: &mut MatcherCursor<'_>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        while self.cursor < self.queue.len() {
            governor.charge(Resource::Work, 1)?;
            cursor.state = self.queue[self.cursor] as usize;
            self.cursor += 1;
            cursor.fallback = self.fail[cursor.state] as usize;
            if FIRST {
                // Read both suffix candidates, select original order, publish.
                governor.charge(Resource::Work, 3)?;
                cursor.current = self.found[cursor.state];
                cursor.inherited = self.found[cursor.fallback];
                self.found.as_mut_slice()[cursor.state] = match (&cursor.current, &cursor.inherited)
                {
                    (Some(own), Some(other)) if own.0 <= other.0 => cursor.current,
                    (Some(_), None) => cursor.current,
                    _ => cursor.inherited,
                };
            } else if self.found[cursor.state].is_none() {
                self.found.as_mut_slice()[cursor.state] = self.found[cursor.fallback];
            }
            let children = &self.goto.nodes[cursor.state].children;
            cursor.comparisons = u64::from(children.len().bit_width()) + 1;
            cursor.byte = 0;
            loop {
                governor.charge(Resource::Work, cursor.comparisons + 3)?;
                cursor.slot = cursor.state * 256 + usize::from(cursor.byte);
                cursor.fallback_state =
                    self.delta[cursor.fallback * 256 + usize::from(cursor.byte)];
                match children.binary_search_by_key(&cursor.byte, |(key, _)| *key) {
                    Ok(position) => {
                        cursor.position = position;
                        cursor.child = children[cursor.position].1;
                        self.fail.as_mut_slice()[cursor.child as usize] = cursor.fallback_state;
                        self.delta.as_mut_slice()[cursor.slot] = cursor.child;
                        self.queue.push(cursor.child)?;
                    }
                    Err(_) => self.delta.as_mut_slice()[cursor.slot] = cursor.fallback_state,
                }
                if cursor.byte == u8::MAX {
                    break;
                }
                cursor.byte += 1;
            }
        }
        Ok(())
    }
}

impl ScratchCharge for Matcher {
    // Three exact scratch owners (40 each) and the portable usize slot (8).
    const CHARGE: usize = 128;
}
impl ScratchCharge for Option<(u32, u32)> {
    const CHARGE: usize = 16;
}
const _: () = assert!(
    core::mem::size_of::<Option<(u32, u32)>>() <= <Option<(u32, u32)> as ScratchCharge>::CHARGE
);
const _: () = assert!(core::mem::size_of::<Matcher>() <= Matcher::CHARGE);

impl Matcher {
    /// Build proper continuation depths only for a caller needing lexical
    /// holdback. Ordinary delimiter matchers retain no additional table.
    pub(crate) fn continuation_depths(
        &self,
        governor: &mut Governor,
    ) -> Result<ScratchVec<u32>, StopReason> {
        let cells = self
            .depth
            .len()
            .checked_mul(256)
            .ok_or_else(|| governor.stop_resource(Resource::ScratchBytes))?;
        if self.delta.len() != cells {
            return Err(invalid_input(
                "matcher continuation table has inconsistent dimensions",
            ));
        }
        // Each finalized transition needs one target lookup, depth lookup,
        // subtraction and maximum update. Insertions admit their own work.
        let work = size(cells)
            .checked_mul(4)
            .ok_or_else(|| governor.stop_resource(Resource::Work))?;
        governor.charge(Resource::Work, work)?;
        let mut values = ScratchVec::new(governor);
        values.reserve(self.depth.len())?;
        for row in self.delta.as_chunks::<256>().0 {
            let mut viable = 0;
            for &next in row {
                let depth = self
                    .depth
                    .get(next as usize)
                    .ok_or_else(|| invalid_input("matcher continuation target is out of range"))?;
                viable = viable.max(depth.saturating_sub(1));
            }
            push_scratch(&mut values, viable, governor)?;
        }
        governor.check()?;
        Ok(values)
    }

    /// Build the matcher; `literals` must be non-empty byte strings.
    pub(crate) fn build<B: AsRef<[u8]>>(
        literals: &[B],
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        Self::build_ordered::<B, false>(literals, governor)
    }

    /// Prefer the first input literal among all suffixes ending together.
    /// The private lease covers real construction frames and return carriers.
    pub(crate) fn build_first<B: AsRef<[u8]>>(
        literals: &[B],
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        const CONTROL: usize = 656;
        // Final projection: owners, working records, input/borrow/return values.
        const {
            assert!(
                core::mem::size_of::<(
                    MatcherBuild,
                    MatcherCursor<'_>,
                    Result<Matcher, StopReason>,
                    &[B],
                    &[u8],
                    &B,
                    &TrieNode,
                    &ScratchVec<(u8, u32)>,
                    Result<usize, usize>,
                    usize,
                    Option<(u32, u32)>,
                    (&(u32, u32), &(u32, u32)),
                    &mut Governor,
                    ScratchLease,
                )>() <= CONTROL
            );
            // Nested trie creation precedes the MatcherBuild owner frame.
            assert!(
                core::mem::size_of::<(
                    MatcherCursor<'_>,
                    TrieBuild<IndexedLiterals<'_, B>>,
                    TrieCursor<'_>,
                    TrieNode,
                    Result<Trie, StopReason>,
                    IndexedLiterals<'_, B>,
                    Option<(&[u8], u32)>,
                    &[B],
                    &B,
                    &ScratchVec<(u8, u32)>,
                    &mut ScratchVec<(u8, u32)>,
                    Result<usize, usize>,
                    Result<u32, core::num::TryFromIntError>,
                    usize,
                    &mut Governor,
                    ScratchLease,
                )>() <= CONTROL
            );
            // Allocation temporarily owns the returned goto and zero iterators.
            assert!(
                core::mem::size_of::<(
                    Result<MatcherBuild, StopReason>,
                    MatcherCursor<'_>,
                    Result<Trie, StopReason>,
                    &[B],
                    &[u8],
                    core::iter::RepeatN<Option<(u32, u32)>>,
                    core::iter::RepeatN<Option<(u32, u32)>>,
                    core::iter::RepeatN<u32>,
                    u64,
                    &mut Governor,
                    ScratchLease,
                )>() <= CONTROL
            );
        }
        governor.charge(Resource::Work, 1)?;
        let _construction = governor.lease_scratch(size(CONTROL))?;
        Self::build_ordered::<B, true>(literals, governor)
    }

    fn build_ordered<B: AsRef<[u8]>, const FIRST: bool>(
        literals: &[B],
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        governor.charge(Resource::Depth, 1)?;
        governor.charge(Resource::Work, size(literals.len()))?;
        u32::try_from(literals.len())
            .map_err(|_| governor.stop_resource(Resource::ScratchBytes))?;
        let mut cursor = MatcherCursor::new();
        while cursor.literal < literals.len() {
            cursor.bytes = literals[cursor.literal].as_ref();
            governor.charge(Resource::InputBytes, size(cursor.bytes.len()))?;
            governor.charge(Resource::Work, size(cursor.bytes.len()).saturating_add(1))?;
            u32::try_from(cursor.bytes.len())
                .map_err(|_| governor.stop_resource(Resource::ScratchBytes))?;
            cursor.longest = cursor.longest.max(cursor.bytes.len());
            cursor.literal += 1;
        }
        // The existing fallible trie is the only goto/prefix authority.
        let mut build = MatcherBuild::allocate(
            Trie::build(IndexedLiterals { literals, next: 0 }, governor)?,
            governor,
        )?;
        build.prefixes(&mut cursor, governor)?;
        build.transitions::<FIRST>(&mut cursor, governor)?;
        governor.check()?;
        Ok(Self {
            delta: build.delta,
            depth: build.depth,
            found: build.found,
            longest: cursor.longest,
        })
    }

    /// The state after `byte`.
    #[inline]
    pub(crate) fn step(&self, state: u32, byte: u8) -> u32 {
        self.delta[state as usize * 256 + usize::from(byte)]
    }

    /// `(literal index, length)` selected by the construction's suffix policy.
    #[inline]
    pub(crate) fn found(&self, state: u32) -> Option<(u32, u32)> {
        self.found[state as usize]
    }

    /// Length of the input suffix `state` represents.
    #[inline]
    pub(crate) fn depth(&self, state: u32) -> u32 {
        self.depth[state as usize]
    }

    /// Length of the longest literal.
    pub(crate) const fn longest(&self) -> usize {
        self.longest
    }
}

#[cfg(test)]
mod tests {
    use super::{Matcher, ROOT, Trie};
    use crate::{Budget, Governor};

    #[test]
    fn matcher_reports_earliest_end_then_longest() {
        let mut governor = Governor::new(Budget::default());
        let literals = vec![
            b"</arg>".to_vec(),
            b"</args>".to_vec(),
            b"g>".to_vec(),
            b"</arg>".to_vec(),
        ];
        let matcher = Matcher::build(&literals, &mut governor).unwrap();
        let mut state = ROOT;
        let mut hits = Vec::new();
        for (offset, &byte) in b"x</args>".iter().enumerate() {
            state = matcher.step(state, byte);
            if let Some((index, length)) = matcher.found(state) {
                hits.push((offset, index, length));
            }
        }
        assert_eq!(hits, [(7, 1, 7)]);
        let mut state = ROOT;
        for &byte in b"</arg" {
            state = matcher.step(state, byte);
        }
        assert_eq!(matcher.depth(state), 5);
        assert_eq!(matcher.found(matcher.step(state, b'>')), Some((0, 6)));

        // Native wrapper-tail order may prefer a shorter opener suffix over
        // a longer closer. Ordinary delimiter matching still keeps the longest.
        let priority = [b"g>".as_slice(), b"</arg>", b"</arg>"];
        let longest = Matcher::build(&priority, &mut governor).unwrap();
        let first = Matcher::build_first(&priority, &mut governor).unwrap();
        let (mut longest_state, mut first_state) = (ROOT, ROOT);
        for &byte in b"</arg>" {
            longest_state = longest.step(longest_state, byte);
            first_state = first.step(first_state, byte);
        }
        assert_eq!(longest.found(longest_state), Some((1, 6)));
        assert_eq!(first.found(first_state), Some((0, 2)));
    }

    #[test]
    fn dense_matcher_admission_releases_partial_tables_and_retains_final_ownership() {
        use crate::contract::{CancellationToken, Resource, StopReason};
        let long = [b'x'; 256];
        let literals = [long.as_slice(), b"tail", b"ai"];
        let mut measured = Governor::new(Budget::default());
        let matcher = Matcher::build(&literals, &mut measured).unwrap();
        let usage = measured.usage();
        let state = long
            .iter()
            .fold(ROOT, |state, byte| matcher.step(state, *byte));
        assert_eq!(matcher.found(state), Some((0, 256)));
        drop(matcher);
        assert_eq!(measured.scratch_meter().live(), 0);
        let exact = Budget {
            work: usage.work,
            scratch_bytes: usage.peak_scratch_bytes,
            input_bytes: 256,
            depth: 1,
            ..Budget::default()
        };
        let mut g = Governor::new(exact);
        drop(Matcher::build(&literals, &mut g).unwrap());
        assert_eq!(g.scratch_meter().live(), 0);
        for (budget, resource) in [
            (
                Budget {
                    work: exact.work - 1,
                    ..exact
                },
                Resource::Work,
            ),
            (
                Budget {
                    scratch_bytes: exact.scratch_bytes - 1,
                    ..exact
                },
                Resource::ScratchBytes,
            ),
            (
                Budget {
                    input_bytes: exact.input_bytes - 1,
                    ..exact
                },
                Resource::InputBytes,
            ),
            (Budget { depth: 0, ..exact }, Resource::Depth),
        ] {
            let mut g = Governor::new(budget);
            assert_eq!(
                Matcher::build(&literals, &mut g).unwrap_err(),
                StopReason::BudgetExceeded(resource)
            );
            assert_eq!(g.check(), Err(StopReason::BudgetExceeded(resource)));
            assert_eq!(g.scratch_meter().live(), 0);
        }
        let token = CancellationToken::new();
        token.cancel();
        let mut g = Governor::with_cancellation(exact, token);
        assert_eq!(
            Matcher::build(&literals, &mut g).unwrap_err(),
            StopReason::Cancelled
        );
        assert_eq!(g.usage().work, 0);
        assert_eq!(g.scratch_meter().live(), 0);
    }

    #[test]
    fn trie_labels_and_admission() {
        let mut governor = Governor::new(Budget::default());
        let trie = Trie::build(
            [(b"</a>".as_slice(), 1), (b"</ab>".as_slice(), 2)],
            &mut governor,
        )
        .unwrap();
        assert!(trie.admits(b"</a"));
        assert!(trie.admits(b"</a>zz"));
        assert!(!trie.admits(b"</x"));
        let mut node = ROOT;
        for &byte in b"</a>" {
            node = trie.step(node, byte).unwrap();
        }
        assert_eq!(trie.accept(node), Some(1));
        assert!(!trie.has_children(node));
        assert_eq!(trie.first_bytes().collect::<Vec<_>>(), [b'<']);
    }

    #[test]
    fn sparse_trie_refusals_release_nodes_and_edges_without_limiting_byte_length() {
        use crate::contract::{CancellationToken, Resource, StopReason};
        let long = [b'x'; 256];
        let spellings = [(b"z".as_slice(), 1), (b"a".as_slice(), 2), (&long, 3)];
        let mut measured = Governor::new(Budget::default());
        let trie = Trie::build(spellings, &mut measured).unwrap();
        let usage = measured.usage();
        let mut node = ROOT;
        for byte in long {
            node = trie.step(node, byte).unwrap();
        }
        assert_eq!(trie.accept(node), Some(3));
        drop(trie);
        assert_eq!(measured.scratch_meter().live(), 0);
        for (budget, resource) in [
            (
                Budget {
                    work: usage.work - 1,
                    ..Budget::default()
                },
                Resource::Work,
            ),
            (
                Budget {
                    scratch_bytes: usage.peak_scratch_bytes - 1,
                    ..Budget::default()
                },
                Resource::ScratchBytes,
            ),
            (
                Budget {
                    depth: 0,
                    ..Budget::default()
                },
                Resource::Depth,
            ),
        ] {
            let mut g = Governor::new(budget);
            let reason = StopReason::BudgetExceeded(resource);
            assert_eq!(Trie::build(spellings, &mut g).unwrap_err(), reason);
            assert_eq!(g.check(), Err(reason));
            assert_eq!(g.scratch_meter().live(), 0);
        }
        let token = CancellationToken::new();
        token.cancel();
        let mut g = Governor::with_cancellation(Budget::default(), token);
        assert_eq!(
            Trie::build(spellings, &mut g).unwrap_err(),
            StopReason::Cancelled
        );
        assert_eq!(g.scratch_meter().live(), 0);
    }
}
