// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Full byte languages over the existing derivative arena/session.

use std::fmt;

use super::derive;
use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchMap, ScratchVec, StopReason, saturating_size as size,
};

/// Postfix construction of a regular byte language, without regex text parsing.
///
/// Classes use little-endian bit positions: byte `b` is bit `b % 64` of word
/// `b / 64`. N-ary concatenation/alternation consume a positive number of roots
/// in source order. Their logical depth is one plus the deepest child, regardless
/// of stack width or the internal binary term representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteInstruction {
    /// Accept only the empty string.
    Empty,
    /// Accept no string.
    Dead,
    /// Accept exactly this byte, including zero or invalid UTF-8.
    Literal(u8),
    /// Accept one byte selected by a fixed 256-bit set.
    Class([u64; 4]),
    /// Concatenate this positive number of preceding roots, in source order.
    Concat(usize),
    /// Union this positive number of preceding roots.
    Alternate(usize),
    /// Zero or more repetitions of the preceding root.
    Star,
}
impl ScratchCharge for ByteInstruction {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 40);
        40
    };
}

/// A malformed postfix program or explicit compilation refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ByteCompileError {
    /// An operator requested zero or more roots than were available.
    Arity {
        /// Original instruction index.
        instruction: usize,
        /// Requested operand count; zero is invalid for an n-ary operator.
        required: usize,
        /// Available roots before the instruction.
        available: usize,
    },
    /// A complete program must leave exactly one root.
    Roots {
        /// Number of roots left by the supplied instructions.
        remaining: usize,
    },
    /// Compilation stopped without publishing a partial language.
    Resource(StopReason),
}
impl fmt::Display for ByteCompileError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "byte-language compilation refused: {self:?}")
    }
}
impl std::error::Error for ByteCompileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resource(reason) => Some(reason),
            _ => None,
        }
    }
}
impl From<StopReason> for ByteCompileError {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}

/// An exact completed inclusion result; resource refusal is a separate error.
#[derive(Debug, PartialEq, Eq)]
pub enum ByteInclusion {
    /// Every left-language byte string belongs to the right language.
    Contained,
    /// Shortest left-only string; ties use ascending unsigned byte order.
    Counterexample(Vec<u8>),
}

/// Caller-retained full-match byte language, using the shared derivative engine.
///
/// Unlike [`super::Pattern`], this operates on arbitrary bytes and never rejoins
/// the root or makes prefix acceptance sticky. No arena-local state IDs escape.
///
/// ```
/// use purrassist_core::{Budget, Governor, pattern::{ByteInstruction as I, ByteLanguage, ByteInclusion}};
/// let mut governor = Governor::new(Budget::default());
/// let byte = ByteLanguage::compile(&[I::Literal(255)], &mut governor).unwrap();
/// let dead = ByteLanguage::compile(&[I::Dead], &mut governor).unwrap();
/// assert!(byte.accepts(&[255], &mut governor).unwrap());
/// assert_eq!(byte.included_in(&dead, &mut governor).unwrap(), ByteInclusion::Counterexample(vec![255]));
/// ```
#[derive(Debug)]
pub struct ByteLanguage {
    compiled: derive::Compiled,
}

impl ByteLanguage {
    /// Compile one postfix program through the existing governed term/class arena.
    ///
    /// Leaf depth is zero. N-ary operators and star add one logical expression
    /// level. Input instruction storage is caller-owned; retained arrays admit
    /// actual capacities before allocation. See the type example.
    ///
    /// # Errors
    /// Returns typed arity/root errors or exact resource/cancellation refusal.
    pub fn compile(
        instructions: &[ByteInstruction],
        governor: &mut Governor,
    ) -> Result<Self, ByteCompileError> {
        Ok(Self {
            compiled: derive::compile_bytes(instructions, governor)?,
        })
    }

    /// Test the complete supplied byte string, without UTF-8 conversion.
    ///
    /// # Errors
    /// Returns exact resource or cancellation refusal, never a partial decision.
    /// The type example demonstrates a non-UTF-8 input.
    pub fn accepts(&self, input: &[u8], governor: &mut Governor) -> Result<bool, StopReason> {
        governor.charge(Resource::InputBytes, size(input.len()))?;
        let mut session = derive::Session::new_bytes(&self.compiled, governor)?;
        let mut state = derive::root(&self.compiled);
        for &byte in input {
            governor.charge(Resource::Work, 1)?;
            state = advance(&mut session, &self.compiled, state, byte, governor)?;
        }
        let accepted = session.nullable_bytes(state, governor)?;
        governor.check()?;
        Ok(accepted)
    }

    /// Prove inclusion or find a shortest deterministic counterexample.
    ///
    /// The product search examines one representative of every joint byte-class
    /// interval. Only a fully exhausted reachable frontier establishes inclusion.
    /// Predecessors retain one byte per discovered state; witness construction is
    /// fallible. Depth bounds witness length, including each discovered prefix.
    /// See the type example for an exact counterexample.
    ///
    /// # Errors
    /// Work, depth, scratch or cancellation failure is an error, never containment.
    pub fn included_in(
        &self,
        other: &Self,
        governor: &mut Governor,
    ) -> Result<ByteInclusion, StopReason> {
        governor.charge(Resource::Depth, 0)?;
        let mut left = derive::Session::new_bytes(&self.compiled, governor)?;
        let mut right = derive::Session::new_bytes(&other.compiled, governor)?;
        let alphabet = representatives(&self.compiled, &other.compiled, governor)?;
        let root = PairNode {
            left: derive::root(&self.compiled),
            right: derive::root(&other.compiled),
            parent: usize::MAX,
            byte: 0,
            depth: 0,
        };
        let mut nodes = ScratchVec::new(governor);
        let mut visited = ScratchMap::new(governor);
        nodes.push(root)?;
        derive::map_work(0, true, governor)?;
        visited.insert((root.left, root.right), ())?;
        let mut next = 0;
        while next < nodes.len() {
            governor.charge(Resource::Work, 1)?;
            let node = nodes.as_slice()[next];
            if left.nullable_bytes(node.left, governor)?
                && !right.nullable_bytes(node.right, governor)?
            {
                return Ok(ByteInclusion::Counterexample(witness(
                    next, &nodes, governor,
                )?));
            }
            for &byte in alphabet.as_slice() {
                governor.charge(Resource::Work, 1)?;
                let a = advance(&mut left, &self.compiled, node.left, byte, governor)?;
                // The canonical dead term rejects every extension, so this
                // branch cannot contain a left-only witness.
                if derive::dead(a) {
                    continue;
                }
                let b = advance(&mut right, &other.compiled, node.right, byte, governor)?;
                derive::map_work(visited.len(), false, governor)?;
                if visited.get(&(a, b)).is_some() {
                    continue;
                }
                let depth = node
                    .depth
                    .checked_add(1)
                    .ok_or(StopReason::BudgetExceeded(Resource::Depth))?;
                governor.charge(Resource::Depth, size(depth))?;
                // Sorted-map insertion compares again and may shift every entry.
                derive::map_work(visited.len(), true, governor)?;
                visited.insert((a, b), ())?;
                nodes.push(PairNode {
                    left: a,
                    right: b,
                    parent: next,
                    byte,
                    depth,
                })?;
            }
            next += 1;
        }
        governor.check()?;
        Ok(ByteInclusion::Contained)
    }
}

fn advance(
    session: &mut derive::Session,
    compiled: &derive::Compiled,
    state: u32,
    byte: u8,
    governor: &mut Governor,
) -> Result<u32, StopReason> {
    let code = u32::from(byte);
    derive::map_work(derive::cuts(compiled).len(), false, governor)?;
    session.step_bytes(
        derive::classes(compiled),
        state,
        code,
        derive::minterm_of(derive::cuts(compiled), code).0,
        governor,
    )
}

fn representatives(
    left: &derive::Compiled,
    right: &derive::Compiled,
    governor: &mut Governor,
) -> Result<ScratchVec<u8>, StopReason> {
    let mut cuts = [false; 257];
    cuts[0] = true;
    cuts[256] = true;
    for &cut in derive::cuts(left).iter().chain(derive::cuts(right)) {
        governor.charge(Resource::Work, 1)?;
        // Only the direct byte compiler constructs these programs: every
        // interval lies in 0..=255, so its cut points lie in 0..=256.
        cuts[cut as usize] = true;
    }
    let mut representatives = ScratchVec::new(governor);
    for byte in 0..=255_u8 {
        governor.charge(Resource::Work, 1)?;
        if cuts[usize::from(byte)] {
            representatives.push(byte)?;
        }
    }
    Ok(representatives)
}

#[derive(Clone, Copy)]
struct PairNode {
    left: u32,
    right: u32,
    parent: usize,
    byte: u8,
    depth: usize,
}
impl ScratchCharge for PairNode {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 32);
        32
    };
}

fn witness(
    mut index: usize,
    nodes: &ScratchVec<PairNode>,
    governor: &mut Governor,
) -> Result<Vec<u8>, StopReason> {
    let mut bytes = ScratchVec::new(governor);
    bytes.reserve(nodes.as_slice()[index].depth)?;
    while nodes.as_slice()[index].parent != usize::MAX {
        governor.charge(Resource::Work, 1)?;
        let node = nodes.as_slice()[index];
        bytes.push(node.byte)?;
        index = node.parent;
    }
    governor.charge(Resource::Work, size(bytes.len()))?;
    bytes.as_mut_slice().reverse();
    governor.check()?;
    Ok(bytes.into_vec())
}
