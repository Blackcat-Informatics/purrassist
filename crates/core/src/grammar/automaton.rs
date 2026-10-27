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
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Matcher {
    delta: ScratchVec<u32>,
    depth: ScratchVec<u32>,
    found: ScratchVec<Option<(u32, u32)>>,
    longest: usize,
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
    /// Build the matcher; `literals` must be non-empty byte strings.
    pub(crate) fn build<B: AsRef<[u8]>>(
        literals: &[B],
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        governor.charge(Resource::Depth, 1)?;
        governor.charge(Resource::Work, size(literals.len()))?;
        u32::try_from(literals.len())
            .map_err(|_| governor.stop_resource(Resource::ScratchBytes))?;
        let mut longest = 0;
        for literal in literals {
            let bytes = literal.as_ref();
            governor.charge(Resource::InputBytes, size(bytes.len()))?;
            governor.charge(Resource::Work, size(bytes.len()).saturating_add(1))?;
            u32::try_from(bytes.len())
                .map_err(|_| governor.stop_resource(Resource::ScratchBytes))?;
            longest = longest.max(bytes.len());
        }
        // The existing fallible trie is the only goto/prefix authority.
        let goto = Trie::build(
            literals.iter().enumerate().map(|(index, literal)| {
                (
                    literal.as_ref(),
                    u32::try_from(index).expect("admitted literal index"),
                )
            }),
            governor,
        )?;
        let count = goto.nodes.len();
        let cells = count
            .checked_mul(256)
            .ok_or_else(|| governor.stop_resource(Resource::ScratchBytes))?;
        // Admit zero initialization of all final and temporary table elements.
        let work = size(cells)
            .checked_add(
                size(count)
                    .checked_mul(3)
                    .ok_or_else(|| governor.stop_resource(Resource::Work))?,
            )
            .ok_or_else(|| governor.stop_resource(Resource::Work))?;
        governor.charge(Resource::Work, work)?;
        let mut delta = ScratchVec::new(governor);
        delta.extend(core::iter::repeat_n(0_u32, cells))?;
        let mut depth = ScratchVec::new(governor);
        depth.extend(core::iter::repeat_n(0_u32, count))?;
        let mut fail = ScratchVec::new(governor);
        fail.extend(core::iter::repeat_n(0_u32, count))?;
        let mut found = ScratchVec::new(governor);
        found.extend(core::iter::repeat_n(None, count))?;
        // Trie insertion always assigns a parent before its children.
        for (state, node) in goto.nodes.iter().enumerate() {
            governor.charge(Resource::Work, size(node.children.len()).saturating_add(1))?;
            for &(_, child) in &node.children {
                let next = depth[state]
                    .checked_add(1)
                    .ok_or_else(|| governor.stop_resource(Resource::ScratchBytes))?;
                depth.as_mut_slice()[child as usize] = next;
            }
            found.as_mut_slice()[state] = node.accept.map(|label| (label, depth[state]));
        }
        let mut queue = ScratchVec::new(governor);
        governor.charge(Resource::Work, size(goto.nodes[0].children.len()))?;
        for &(byte, child) in &goto.nodes[0].children {
            delta.as_mut_slice()[usize::from(byte)] = child;
            queue.push(child)?;
        }
        let mut cursor = 0;
        while let Some(&state) = queue.as_slice().get(cursor) {
            governor.charge(Resource::Work, 1)?;
            cursor += 1;
            let state = state as usize;
            let fallback = fail[state] as usize;
            if found[state].is_none() {
                found.as_mut_slice()[state] = found[fallback];
            }
            let children = &goto.nodes[state].children;
            let comparisons = u64::from(children.len().bit_width()) + 1;
            for byte in 0..=255_u8 {
                governor.charge(Resource::Work, comparisons + 3)?;
                let slot = state * 256 + usize::from(byte);
                let fallback_state = delta[fallback * 256 + usize::from(byte)];
                match children.binary_search_by_key(&byte, |(key, _)| *key) {
                    Ok(position) => {
                        let child = children[position].1;
                        fail.as_mut_slice()[child as usize] = fallback_state;
                        delta.as_mut_slice()[slot] = child;
                        queue.push(child)?;
                    }
                    Err(_) => delta.as_mut_slice()[slot] = fallback_state,
                }
            }
        }
        governor.check()?;
        Ok(Self {
            delta,
            depth,
            found,
            longest,
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
