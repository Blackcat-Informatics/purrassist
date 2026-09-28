// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The parsed surface syntax tree, shared by the backtracking evaluator
//! (`backtrack.rs`, which matches it directly) and the derivative engine
//! (`derive.rs`, which lowers the backreference/lookaround-free subset into
//! its own bounded-size arena representation).
use std::rc::Rc;

/// A sorted, disjoint, non-adjacent-merged set of Unicode scalar values, and
/// the unit every character class (literal, `.`, `\d`/`\w`/`\s` and their
/// negations, `[...]`, and every `\p{...}`/`\P{...}` property) compiles to.
/// Negation is resolved once, at construction, against
/// [`SCALAR_DOMAIN`](super::unicode::SCALAR_DOMAIN) (which excludes the
/// surrogate range, since a Rust `char` can never hold one), so matching is
/// always "is this scalar in the set", never "is it not excluded".
pub(super) type Intervals = Rc<Vec<(u32, u32)>>;

/// The parsed syntax tree for one pattern.
///
/// `Group`, `Backref`, and `Look` are meaningful only to the backtracking
/// evaluator (`backtrack.rs`); the derivative engine (`derive.rs`) only ever
/// receives a tree built from a pattern containing neither `Backref` nor
/// `Look` (checked once, in `Pattern::compile_governed`), and treats `Group`
/// as fully transparent since matching is boolean (no capture output).
#[derive(Clone, Debug)]
pub(super) enum Node {
    /// Matches the empty string.
    Empty,
    /// Matches nothing at all (`[^\s\S]`, or an intersection with its own complement).
    NoMatch,
    /// Matches exactly one scalar value in this set.
    Class(Intervals),
    Concat(Vec<Self>),
    Alternate(Vec<Self>),
    Repeat(Box<Repetition>),
    /// A capturing group; index 0 is never used (capture indices start at 1).
    Group(usize, Box<Self>),
    Start,
    End,
    /// `true` for `\b`, `false` for `\B`.
    Boundary(bool),
    Look {
        inner: Box<Self>,
        positive: bool,
        behind: bool,
    },
    /// A resolved backreference: a numeric reference resolves to exactly
    /// one group index; a named reference resolves to every group index
    /// sharing that name (more than one only when the name is legitimately
    /// duplicated across mutually exclusive alternatives — ECMA-262's rule
    /// for which this ties the runtime resolution to whichever of them
    /// actually participated in the match; see `backtrack.rs`).
    Backref(Vec<usize>),
}
#[derive(Clone, Debug)]
pub(super) struct Repetition {
    pub(super) inner: Node,
    pub(super) min: usize,
    pub(super) max: Option<usize>,
    pub(super) greedy: bool,
}

impl Node {
    /// `true` if this subtree contains a backreference or a lookaround,
    /// which only the backtracking evaluator supports.
    pub(super) fn requires_backtracking(&self) -> bool {
        match self {
            Self::Empty
            | Self::NoMatch
            | Self::Class(_)
            | Self::Start
            | Self::End
            | Self::Boundary(_) => false,
            Self::Backref(_) | Self::Look { .. } => true,
            Self::Concat(items) | Self::Alternate(items) => {
                items.iter().any(Self::requires_backtracking)
            }
            Self::Repeat(repetition) => repetition.inner.requires_backtracking(),
            Self::Group(_, inner) => inner.requires_backtracking(),
        }
    }
}
