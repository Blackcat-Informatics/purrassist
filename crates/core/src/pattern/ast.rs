// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The parsed surface syntax tree, shared by the backtracking evaluator
//! (`backtrack.rs`, which matches it directly) and the derivative engine
//! (`derive.rs`, which lowers the backreference/lookaround-free subset into
//! its own bounded-size arena representation).
use crate::contract::{ScratchCharge, ScratchVec};

/// A sorted, disjoint, non-adjacent-merged set of Unicode scalar values, and
/// the unit every character class (literal, `.`, `\d`/`\w`/`\s` and their
/// negations, `[...]`, and every `\p{...}`/`\P{...}` property) compiles to.
/// Negation is resolved once, at construction, against
/// [`SCALAR_DOMAIN`](super::unicode::SCALAR_DOMAIN) (which excludes the
/// surrogate range, since a Rust `char` can never hold one), so matching is
/// always "is this scalar in the set", never "is it not excluded".
pub(super) type Intervals = ScratchVec<(u32, u32)>;

/// The parsed syntax tree for one pattern.
///
/// `Group`, `Backref`, and `Look` are meaningful only to the backtracking
/// evaluator (`backtrack.rs`); the derivative engine (`derive.rs`) only ever
/// receives a tree built from a pattern containing neither `Backref` nor
/// `Look` (checked once, in `Pattern::compile_governed`), and treats `Group`
/// as fully transparent since matching is boolean (no capture output).
#[derive(Debug)]
pub(super) enum Node {
    /// Matches the empty string.
    Empty,
    /// Matches nothing at all (`[^\s\S]`, or an intersection with its own complement).
    NoMatch,
    /// Matches exactly one scalar value in this set.
    Class(Intervals),
    Concat(ScratchVec<Self>),
    Alternate(ScratchVec<Self>),
    Repeat(ScratchVec<Repetition>),
    /// A capturing group; index 0 is never used (capture indices start at 1).
    Group(usize, ScratchVec<Self>),
    Start,
    End,
    /// `true` for `\b`, `false` for `\B`.
    Boundary(bool),
    Look {
        inner: ScratchVec<Self>,
        positive: bool,
        behind: bool,
    },
    /// A resolved backreference: a numeric reference resolves to exactly
    /// one group index; a named reference resolves to every group index
    /// sharing that name (more than one only when the name is legitimately
    /// duplicated across mutually exclusive alternatives — ECMA-262's rule
    /// for which this ties the runtime resolution to whichever of them
    /// actually participated in the match; see `backtrack.rs`).
    Backref(ScratchVec<usize>),
}
#[derive(Debug)]
pub(super) struct Repetition {
    pub(super) inner: Node,
    pub(super) min: usize,
    pub(super) max: Option<usize>,
    pub(super) greedy: bool,
}

impl ScratchCharge for Node {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 64);
        64
    };
}
impl ScratchCharge for Repetition {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 96);
        96
    };
}
