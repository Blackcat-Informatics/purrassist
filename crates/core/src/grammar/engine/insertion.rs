// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The source-contiguous suffix ending at an inserted wrapper closer.

use super::{Arena, HistKind};
use crate::contract::saturating_size as size;
use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchMap, ScratchVec, StopReason, invalid_input,
    push_scratch,
};
use crate::grammar::cost::K_THREAD;
use crate::grammar::edit::EditOperator;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Boundary {
    at: usize,
    /// Every transparent insertion must share the wrapper's own boundary.
    inserted: Option<usize>,
}

impl ScratchCharge for Boundary {
    const CHARGE: usize = 24;
}

#[derive(Clone, Copy)]
enum Rewrite {
    Guess,
    Absorb { end: usize },
}

/// Only certified tail nodes may be rewritten. Their increasing arena indices
/// order children before parents; captures, values and links never enter it.
pub(super) struct InsertionTail {
    root: u32,
    nodes: ScratchMap<u32, Boundary>,
}

const _: () = assert!(core::mem::size_of::<Boundary>() <= Boundary::CHARGE);

/// Every packed branch must reach its wrapper without unrecorded source input.
/// Explicit trailing tokens are transparent only at that same source boundary.
pub(super) fn ending(
    arena: &Arena,
    hist: u32,
    end: usize,
    governor: &mut Governor,
) -> Result<Option<InsertionTail>, StopReason> {
    certify(arena, hist, end, true, governor)
}

/// Undo candidates cannot contain an already consumed stray closer: the
/// wrapper would then precede this absorption's source start. Stop immediately
/// rather than repeatedly scanning a growing stray-closer history.
pub(super) fn absorbing(
    arena: &Arena,
    hist: u32,
    end: usize,
    governor: &mut Governor,
) -> Result<Option<InsertionTail>, StopReason> {
    certify(arena, hist, end, false, governor)
}

/// Ordinary completed captures and unrelated edits need no suffix owner.
/// Absorption also stops at a previous stray in constant work.
fn tail_kind(
    arena: &Arena,
    hist: u32,
    strays: bool,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(Resource::Work, K_THREAD)?;
    Ok(match arena.nodes[hist as usize].kind {
        HistKind::Pack(_) => true,
        HistKind::Edit(index) => {
            let operator = arena.edits[index as usize].operator;
            matches!(
                operator,
                EditOperator::MissingWrapperClose | EditOperator::MissingToken
            ) || strays && operator == EditOperator::DeleteStrayCloser
        }
        _ => false,
    })
}

fn certify(
    arena: &Arena,
    hist: u32,
    end: usize,
    strays: bool,
    governor: &mut Governor,
) -> Result<Option<InsertionTail>, StopReason> {
    if !tail_kind(arena, hist, strays, governor)? {
        return Ok(None);
    }
    let mut pending = ScratchVec::new(governor);
    let mut nodes = ScratchMap::new(governor);
    push_scratch(
        &mut pending,
        (
            hist,
            Boundary {
                at: end,
                inserted: None,
            },
        ),
        governor,
    )?;
    while let Some((at, boundary)) = pending.pop() {
        governor.charge(Resource::Work, K_THREAD)?;
        governor.charge(Resource::Work, u64::from(nodes.len().bit_width() + 1))?;
        if let Some(previous) = nodes.get(&at) {
            if previous.at != boundary.at
                || previous
                    .inserted
                    .zip(boundary.inserted)
                    .is_some_and(|(left, right)| left != right)
            {
                return Ok(None);
            }
            // A shared suffix may arrive directly and through an explicit
            // token. Revisit only when the latter adds its stronger same-
            // boundary obligation; the weaker path cannot invalidate it.
            if previous.inserted.is_some() || boundary.inserted.is_none() {
                continue;
            }
        }
        governor.charge(
            Resource::Work,
            size(nodes.len()).saturating_mul(2).saturating_add(1),
        )?;
        nodes.insert(at, boundary)?;
        let node = arena.nodes[at as usize];
        match node.kind {
            HistKind::Edit(index) => {
                let edit = &arena.edits[index as usize];
                match edit.operator {
                    EditOperator::MissingWrapperClose
                        if edit.span.start == boundary.at
                            && edit.span.end == boundary.at
                            && edit.original.is_empty()
                            && !edit.replacement.is_empty()
                            && boundary.inserted.is_none_or(|at| at == boundary.at) => {}
                    EditOperator::MissingToken
                        if edit.span.start == boundary.at
                            && edit.span.end == boundary.at
                            && edit.original.is_empty()
                            && !edit.replacement.is_empty()
                            && boundary.inserted.is_none_or(|at| at == boundary.at) =>
                    {
                        push_scratch(
                            &mut pending,
                            (
                                node.prev,
                                Boundary {
                                    inserted: Some(boundary.at),
                                    ..boundary
                                },
                            ),
                            governor,
                        )?;
                    }
                    EditOperator::DeleteStrayCloser if strays && edit.span.end == boundary.at => {
                        push_scratch(
                            &mut pending,
                            (
                                node.prev,
                                Boundary {
                                    at: edit.span.start,
                                    ..boundary
                                },
                            ),
                            governor,
                        )?;
                    }
                    _ => return Ok(None),
                }
            }
            HistKind::Pack(other) => {
                push_scratch(&mut pending, (node.prev, boundary), governor)?;
                push_scratch(&mut pending, (other, boundary), governor)?;
            }
            _ => return Ok(None),
        }
    }
    Ok(Some(InsertionTail { root: hist, nodes }))
}

impl InsertionTail {
    /// Relabel only the proved wrapper, preserving explicit token insertions
    /// and shared histories belonging to other completions.
    pub(super) fn guess(
        &self,
        arena: &mut Arena,
        governor: &mut Governor,
    ) -> Result<u32, StopReason> {
        self.rewrite(arena, Rewrite::Guess, governor)
    }

    /// Undo only a wrapper whose exact replacement now appears as source.
    /// Bracket insertions above it remain edits in their original order.
    pub(super) fn absorb(
        &self,
        arena: &mut Arena,
        start: usize,
        end: usize,
        original: &[u8],
        governor: &mut Governor,
    ) -> Result<Option<u32>, StopReason> {
        for &(at, _) in self.nodes.iter() {
            governor.charge(Resource::Work, K_THREAD)?;
            let HistKind::Edit(index) = arena.nodes[at as usize].kind else {
                continue;
            };
            let edit = &arena.edits[index as usize];
            if edit.operator == EditOperator::MissingWrapperClose {
                governor.charge(Resource::Work, size(original.len()))?;
                if edit.span.start != start
                    || original.trim_ascii_start() != edit.replacement.as_slice()
                {
                    return Ok(None);
                }
            }
        }
        self.rewrite(arena, Rewrite::Absorb { end }, governor)
            .map(Some)
    }

    fn rewrite(
        &self,
        arena: &mut Arena,
        rewrite: Rewrite,
        governor: &mut Governor,
    ) -> Result<u32, StopReason> {
        let mut rewritten = ScratchMap::new(governor);
        for &(at, _) in self.nodes.iter() {
            governor.charge(Resource::Work, K_THREAD)?;
            let node = arena.nodes[at as usize];
            let mapped = |child| {
                rewritten
                    .get(&child)
                    .copied()
                    .ok_or_else(|| invalid_input("certified wrapper tail is not in source order"))
            };
            governor.charge(
                Resource::Work,
                u64::from(rewritten.len().bit_width() + 1).saturating_mul(2),
            )?;
            let changed = match node.kind {
                HistKind::Edit(index) => {
                    let edit = &arena.edits[index as usize];
                    let wrapper = edit.operator == EditOperator::MissingWrapperClose;
                    match (wrapper, edit.operator, rewrite) {
                        (true, _, Rewrite::Absorb { .. }) => node.prev,
                        (true, _, Rewrite::Guess)
                        | (false, EditOperator::MissingToken, Rewrite::Absorb { .. }) => {
                            let prev = if wrapper {
                                node.prev
                            } else {
                                mapped(node.prev)?
                            };
                            governor.charge(Resource::Work, edit.estimated_bytes())?;
                            governor.charge(Resource::ScratchBytes, edit.estimated_bytes())?;
                            let mut edit = edit.clone();
                            match rewrite {
                                Rewrite::Guess => {
                                    edit.operator = EditOperator::MissingWrapperCloseBeforeText;
                                }
                                Rewrite::Absorb { end } => {
                                    // The explicit trailing token follows the
                                    // now-consumed wrapper in the source ledger.
                                    edit.span = end..end;
                                }
                            }
                            arena.adopt_edit(prev, edit)?
                        }
                        _ => arena.push(mapped(node.prev)?, node.kind)?,
                    }
                }
                HistKind::Pack(other) => arena.pack(mapped(node.prev)?, mapped(other)?)?,
                _ => {
                    return Err(invalid_input(
                        "certified wrapper tail changed before rewriting",
                    ));
                }
            };
            governor.charge(
                Resource::Work,
                size(rewritten.len()).saturating_mul(2).saturating_add(1),
            )?;
            rewritten.insert(at, changed)?;
        }
        governor.charge(Resource::Work, u64::from(rewritten.len().bit_width() + 1))?;
        rewritten
            .get(&self.root)
            .copied()
            .ok_or_else(|| invalid_input("certified wrapper tail lost its root"))
    }
}
