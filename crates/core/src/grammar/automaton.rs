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
//! scratch ownership. Dense matcher construction/storage retain their existing
//! coarser accounting contract and remain separate cold-governance scope.
// `unreachable_pub` and `redundant_pub_crate` disagree on the right
// visibility for items of a crate-private module
// (https://github.com/rust-lang/rust-clippy/issues/6188); these items are
// shared across the grammar and tool-call modules, so `pub(crate)` (matching
// `unreachable_pub`) wins, as in `pattern::derive`.
#![allow(clippy::redundant_pub_crate)]
use super::cost::K_COMPILE;
use super::index_u32;
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, ScratchCharge, ScratchVec, StopReason};

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
        let mut nodes = ScratchVec::new(governor);
        nodes.push(TrieNode::new(governor))?;
        for (bytes, label) in spellings {
            governor.charge(Resource::Work, K_COMPILE)?;
            let mut node = ROOT as usize;
            for &byte in bytes {
                let children = &nodes[node].children;
                let comparisons = u64::from(children.len().bit_width()) + 1;
                governor.charge(Resource::Work, comparisons * K_COMPILE)?;
                node = match children.binary_search_by_key(&byte, |(key, _)| *key) {
                    Ok(index) => children[index].1 as usize,
                    Err(index) => {
                        let next = nodes.len();
                        let id = u32::try_from(next)
                            .map_err(|_| governor.stop_resource(Resource::ScratchBytes))?;
                        governor
                            .charge(Resource::Work, size(children.len() - index + 1) * K_COMPILE)?;
                        // Fallible growth precedes allocation-free sorted insertion.
                        nodes.push(TrieNode::new(governor))?;
                        let children = &mut nodes.as_mut_slice()[node].children;
                        children.push((byte, id))?;
                        children.as_mut_slice()[index..].rotate_right(1);
                        next
                    }
                };
            }
            nodes.as_mut_slice()[node].accept.get_or_insert(label);
        }
        governor.check()?;
        Ok(Self { nodes })
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
/// After each byte, [`Matcher::found`] reports the longest literal that
/// ends at that byte (so the earliest-ending occurrence wins, and among
/// literals ending together the one starting first), and
/// [`Matcher::depth`] the length of the longest suffix that is still a
/// proper prefix of some literal: the bytes a caller must hold back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Matcher {
    delta: Vec<u32>,
    depth: Vec<u32>,
    found: Vec<Option<(u32, u32)>>,
    longest: usize,
}

impl Matcher {
    /// Build the matcher; `literals` must be non-empty byte strings.
    pub(crate) fn build<B: AsRef<[u8]>>(
        literals: &[B],
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        let total: usize = literals.iter().map(|literal| literal.as_ref().len()).sum();
        let states = total.saturating_add(1);
        governor.charge(
            Resource::Work,
            size(states).saturating_mul(256).saturating_mul(K_COMPILE),
        )?;
        governor.charge(
            Resource::ScratchBytes,
            size(states).saturating_mul(256 * 4 + 32),
        )?;
        // Goto function as a sparse trie first.
        let mut goto: Vec<Vec<(u8, u32)>> = vec![Vec::new()];
        let mut depth = vec![0_u32];
        let mut own: Vec<Option<(u32, u32)>> = vec![None];
        for (index, literal) in literals.iter().enumerate() {
            let literal = literal.as_ref();
            let mut node = 0_usize;
            for &byte in literal {
                node = match goto[node].binary_search_by_key(&byte, |(key, _)| *key) {
                    Ok(position) => goto[node][position].1 as usize,
                    Err(position) => {
                        let next = goto.len();
                        goto[node].insert(position, (byte, index_u32(next)));
                        goto.push(Vec::new());
                        depth.push(depth[node] + 1);
                        own.push(None);
                        next
                    }
                };
            }
            let length = index_u32(literal.len());
            own[node].get_or_insert_with(|| (index_u32(index), length));
        }
        let count = goto.len();
        let mut delta = vec![0_u32; count * 256];
        let mut fail = vec![0_u32; count];
        let mut found: Vec<Option<(u32, u32)>> = own.clone();
        let mut queue = std::collections::VecDeque::new();
        for &(byte, child) in &goto[0] {
            delta[usize::from(byte)] = child;
            queue.push_back(child as usize);
        }
        while let Some(state) = queue.pop_front() {
            let fallback = fail[state] as usize;
            // Longest literal ending here: this state's own, else the
            // fail chain's (already final, since BFS visits shallower first).
            if found[state].is_none() {
                found[state] = found[fallback];
            }
            for byte in 0..=255_u8 {
                let slot = state * 256 + usize::from(byte);
                match goto[state].binary_search_by_key(&byte, |(key, _)| *key) {
                    Ok(position) => {
                        let child = goto[state][position].1 as usize;
                        fail[child] = delta[fallback * 256 + usize::from(byte)];
                        delta[slot] = index_u32(child);
                        queue.push_back(child);
                    }
                    Err(_) => delta[slot] = delta[fallback * 256 + usize::from(byte)],
                }
            }
        }
        Ok(Self {
            delta,
            depth,
            found,
            longest: literals
                .iter()
                .map(|literal| literal.as_ref().len())
                .max()
                .unwrap_or(0),
        })
    }

    /// The state after `byte`.
    #[inline]
    pub(crate) fn step(&self, state: u32, byte: u8) -> u32 {
        self.delta[state as usize * 256 + usize::from(byte)]
    }

    /// `(literal index, length)` of the longest literal ending at `state`.
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
        let literals = vec![b"</arg>".to_vec(), b"</args>".to_vec(), b"g>".to_vec()];
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
