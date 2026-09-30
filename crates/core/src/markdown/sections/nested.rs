// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Section paths and container-aware body insertion.
use super::{Block, BlockKind, Document, shift, size};
use crate::contract::invalid_input as invalid;
use crate::markdown::{line_start, logical_text};
use crate::{
    Governor, StopReason,
    contract::Resource,
    digest::Digest,
    edits::{EditPlan, TextEdit},
};
use std::ops::Range;

/// A source-bound section within a document, quote, or list item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionSite {
    /// Digest of the complete original document.
    pub source: Digest,
    /// Root block index followed by child indices, ending at a heading.
    pub path: Vec<usize>,
    /// Heading and body bounding range in the original source.
    pub span: Range<usize>,
    /// Body range, including complete outer prefixes on its physical lines.
    pub body: Range<usize>,
}

impl Document {
    /// Inspect all root and nested section occurrences in source order.
    ///
    /// Sections stop at a sibling heading of equal or higher rank, or at the end
    /// of their immediate container. Nested container headings do not close an
    /// outer section. Paths are valid only for this document's digest.
    ///
    /// # Errors
    /// Returns cancellation and work/storage/depth exhaustion before returning
    /// any partial occurrence list.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::Document};
    /// let mut g = Governor::new(Budget::default());
    /// let doc = Document::parse("> # Tea\n> Cold.\n", &mut g)?;
    /// assert_eq!(doc.section_sites(&mut g)?[0].path, [0, 0]);
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    pub fn section_sites(&self, governor: &mut Governor) -> Result<Vec<SectionSite>, StopReason> {
        governor.charge(
            Resource::Work,
            size(self.source.len()).saturating_mul(16).saturating_add(1),
        )?;
        governor.charge(
            Resource::ScratchBytes,
            size(self.source.len())
                .saturating_mul(64)
                .saturating_add(256),
        )?;
        let mut result = Vec::new();
        collect(
            self,
            &self.blocks,
            self.source.len(),
            &mut Vec::new(),
            &mut result,
            governor,
        )?;
        Ok(result)
    }

    /// Replace a root or nested section's logical Markdown body.
    ///
    /// Root paths use [`Self::replace_section_governed`]. Nested bodies omit outer
    /// container markers; inserted lines receive canonical quote prefixes and
    /// list-continuation indentation. Original bytes outside the body remain exact.
    /// Newline spelling comes from `body`; callers supply required separators.
    /// An identical logical body leaves its existing prefix spelling untouched.
    /// Reparse checks the intended body structure and every outside sibling and
    /// ancestor. Plans support exact source-bound undo.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::Document};
    /// let mut g = Governor::new(Budget::default());
    /// let doc = Document::parse("> # Tea\n> Hot.\n> # Keep\n> Tail\n", &mut g)?;
    /// let plan = doc.replace_section_at(&[0, 0], "Cold.\n", &mut g)?;
    /// assert_eq!(plan.edits[0].replacement, "> Cold.\n");
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects non-heading paths, changed section/container boundaries, or bodies
    /// whose structure changes under container prefixes. Trailing blank lines
    /// that escape a list item are rejected. Resource stops propagate unchanged.
    pub fn replace_section_at(
        &self,
        path: &[usize],
        body: &str,
        governor: &mut Governor,
    ) -> Result<EditPlan, StopReason> {
        governor.charge(Resource::Work, size(path.len()).saturating_add(1))?;
        governor.charge(Resource::Depth, size(path.len().saturating_sub(1)))?;
        self.block_at(path)?;
        if path.len() == 1 {
            return self.replace_section_governed(path[0], body, governor);
        }
        governor.charge(Resource::InputBytes, size(body.len()))?;
        governor.charge(
            Resource::Work,
            size(self.source.len())
                .saturating_mul(32)
                .saturating_add(size(body.len()).saturating_mul(8)),
        )?;
        governor.charge(
            Resource::ScratchBytes,
            size(self.source.len())
                .saturating_mul(192)
                .saturating_add(size(body.len()).saturating_mul(128))
                .saturating_add(512),
        )?;
        let parent = self.block_at(&path[..path.len() - 1])?;
        let heading = path[path.len() - 1];
        let (range, stop) = bounds(&self.source, &parent.children, heading, parent.span.end)?;
        let mut logical = String::new();
        for block in &parent.children[heading + 1..stop] {
            for (line, padding) in block.lines.iter().zip(&block.line_padding) {
                logical.push_str(&" ".repeat(*padding));
                logical.push_str(&self.source[line.clone()]);
            }
        }
        // Layout recovery: a leading tab is column-dependent (the same byte
        // means a different number of columns depending on where the parent
        // container's prefix leaves off), so a caller-supplied leading tab is
        // re-indented to the number of spaces it would occupy starting at
        // column zero -- exactly how `Self::parse(body, ..)` alone interprets
        // it -- rather than embedded literally and left to whatever column
        // the container prefix happens to end on.
        let recovered_body = expand_leading_tabs_in_body(body);
        if logical == recovered_body {
            return Ok(EditPlan {
                source: self.digest,
                edits: Vec::new(),
            });
        }
        let own_prefix = prefix(self, path, governor)?;
        governor.charge(
            Resource::Work,
            size(own_prefix.len()).saturating_mul(size(recovered_body.len())),
        )?;
        governor.charge(
            Resource::ScratchBytes,
            size(own_prefix.len())
                .saturating_mul(size(recovered_body.len()))
                .saturating_mul(4),
        )?;
        let intended = Self::parse(&recovered_body, governor)?;
        // Layout recovery: a body's trailing blank line(s) never stay nested
        // inside a container that ends immediately afterward -- a blank line
        // followed by a dedent always separates from its container, by this
        // parser's own rule (see `containers::item` trailing-blank handling)
        // -- so a trailing blank body is re-indented to the *container's*
        // level (the same level the container's own trailing blank line
        // would already occupy) instead of being folded into the container's
        // body and rejected as a structural mismatch. Only attempted when the
        // heading's body already extends to the end of its container ( `stop
        // == parent.children.len()`); a body followed by further siblings
        // inside the same container has no adjacent dedent to eject into.
        let (core_replacement, replacement, ejected) = build_recovered_replacement(
            self,
            path,
            parent,
            stop,
            &own_prefix,
            &recovered_body,
            &intended,
            governor,
        )?;
        let mut proposed = self.source.clone();
        proposed.replace_range(range.clone(), &replacement);
        let updated = Self::parse(&proposed, governor)?;
        let new_core_end = range.start + core_replacement.len();
        let new_end = range.start + replacement.len();
        let new_parent = updated.block_at(&path[..path.len() - 1])?;
        let (new_range, new_stop) = bounds(
            &proposed,
            &new_parent.children,
            heading,
            new_parent.span.end,
        )?;
        if new_range != (range.start..new_core_end) {
            return Err(invalid("replacement changes nested section boundaries"));
        }
        if !same_shape(
            &intended.source,
            &intended.blocks[..intended.blocks.len() - ejected],
            &updated.source,
            &new_parent.children[heading + 1..new_stop],
        ) {
            return Err(invalid(
                "replacement changes body structure inside its container",
            ));
        }
        validate_context(
            self,
            &updated,
            path,
            stop,
            new_stop,
            &range,
            new_core_end,
            new_end,
            ejected,
        )?;
        Ok(EditPlan {
            source: self.digest,
            edits: vec![TextEdit { range, replacement }],
        })
    }
}

/// Render every physical line of `body` with `prefix` prepended, exactly as
/// this document's own container prefixes are always spelled.
/// Split `recovered_body` at its trailing-blank boundary (if [`Self`]'s
/// [`replace_section_at`] should eject those lines to the container's own
/// level) and render both halves with their respective container prefixes.
/// Returns `(core_replacement, full_replacement, ejected_blank_count)`.
///
/// [`Self`]: Document
/// [`replace_section_at`]: Document::replace_section_at
#[expect(
    clippy::too_many_arguments,
    reason = "each parameter is an independent fact this one governed reparse-and-split step \
              needs; grouping them into a struct would only move the same eight facts, not \
              reduce them"
)]
fn build_recovered_replacement(
    doc: &Document,
    path: &[usize],
    parent: &Block,
    stop: usize,
    own_prefix: &str,
    recovered_body: &str,
    intended: &Document,
    governor: &mut Governor,
) -> Result<(String, String, usize), StopReason> {
    let trailing_blanks = trailing_blank_run(&intended.blocks);
    let eject = trailing_blanks > 0 && stop == parent.children.len();
    let (core_body, trailing_body) = if eject {
        let split = intended.blocks[intended.blocks.len() - trailing_blanks]
            .span
            .start;
        (&recovered_body[..split], &recovered_body[split..])
    } else {
        (recovered_body, "")
    };
    let outer_prefix = if eject {
        prefix(doc, &path[..path.len() - 1], governor)?
    } else {
        String::new()
    };
    let core_replacement = prefixed_lines(core_body, own_prefix);
    let mut replacement = core_replacement.clone();
    replacement.push_str(&prefixed_lines(trailing_body, &outer_prefix));
    let ejected = if eject { trailing_blanks } else { 0 };
    Ok((core_replacement, replacement, ejected))
}
fn prefixed_lines(body: &str, prefix: &str) -> String {
    let mut result = String::with_capacity(body.len() + prefix.len().saturating_mul(4));
    for line in crate::markdown::lines(body) {
        result.push_str(prefix);
        result.push_str(&body[line]);
    }
    result
}

/// Count the trailing run of top-level `Blank` blocks.
fn trailing_blank_run(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .rev()
        .take_while(|block| matches!(block.kind, BlockKind::Blank))
        .count()
}

/// Expand a physical line's leading tab/space run into spaces, using 4-column
/// tab stops counted from column zero (not from wherever a container prefix
/// would leave the real cursor): matches how [`Document::parse`] alone
/// interprets the same bytes, so a recovered body reparses into the same
/// shape `Document::parse(body, ..)` reports as intended.
fn expand_leading_tabs(line: &str) -> String {
    let mut column = 0_usize;
    let mut end = 0_usize;
    for byte in line.bytes() {
        match byte {
            b' ' => column += 1,
            b'\t' => column += 4 - column % 4,
            _ => break,
        }
        end += 1;
    }
    if !line[..end].contains('\t') {
        return line.into();
    }
    let mut result = " ".repeat(column);
    result.push_str(&line[end..]);
    result
}
fn expand_leading_tabs_in_body(body: &str) -> String {
    let mut result = String::with_capacity(body.len());
    for line in crate::markdown::lines(body) {
        result.push_str(&expand_leading_tabs(&body[line]));
    }
    result
}

/// `new_core_end` is where the heading's own container (`path[..path.len() -
/// 1]`) now ends: equal to `new_end` unless `ejected` trailing `Blank`
/// blocks were pushed out to become that container's own trailing siblings
/// (see the "Layout recovery" comment in [`Document::replace_section_at`]),
/// in which case `new_core_end < new_end` and `ejected` of those siblings
/// are expected immediately after that container in *its* parent.
#[expect(
    clippy::too_many_arguments,
    reason = "distinguishes the container's own shrunk span from the full replaced range \
              exactly where trailing blanks are ejected one level up; splitting this into a \
              struct would only move the same six related facts, not reduce them"
)]
fn validate_context(
    original: &Document,
    updated: &Document,
    path: &[usize],
    stop: usize,
    new_stop: usize,
    range: &Range<usize>,
    new_core_end: usize,
    new_end: usize,
    ejected: usize,
) -> Result<(), StopReason> {
    let mut before = original.blocks.as_slice();
    let mut after = updated.blocks.as_slice();
    for (depth, &index) in path.iter().enumerate() {
        let old = before
            .get(index)
            .ok_or_else(|| invalid("missing original ancestor"))?;
        let new = after
            .get(index)
            .ok_or_else(|| invalid("replacement removes ancestor"))?;
        if before[..index] != after[..index] {
            return Err(invalid("replacement changes preceding blocks"));
        }
        if depth + 1 == path.len() {
            if old != new || !same_tail(&before[stop..], &after[new_stop..], range.end, new_end) {
                return Err(invalid("replacement changes heading or following blocks"));
            }
        } else if depth + 2 == path.len() {
            // This is the heading's direct container's own entry in *its*
            // parent: the one place ejected trailing blanks can appear.
            if old.kind != new.kind
                || old.span.start != new.span.start
                || old.span.end - range.end + new_core_end != new.span.end
                || !same_tail_with_ejected_blanks(
                    &before[index + 1..],
                    &after[index + 1..],
                    range.end,
                    new_end,
                    ejected,
                )
            {
                return Err(invalid("replacement changes ancestor or outside siblings"));
            }
            before = &old.children;
            after = &new.children;
        } else {
            if old.kind != new.kind
                || old.span.start != new.span.start
                || old.span.end - range.end + new_end != new.span.end
                || !same_tail(
                    &before[index + 1..],
                    &after[index + 1..],
                    range.end,
                    new_end,
                )
            {
                return Err(invalid("replacement changes ancestor or outside siblings"));
            }
            before = &old.children;
            after = &new.children;
        }
    }
    Ok(())
}
/// Like [`same_tail`], but the new tail may begin with `ejected` extra
/// `Blank` blocks that have no counterpart in `old` (see [`validate_context`]).
fn same_tail_with_ejected_blanks(
    old: &[Block],
    new: &[Block],
    old_end: usize,
    new_end: usize,
    ejected: usize,
) -> bool {
    if new.len() < ejected {
        return false;
    }
    let (inserted, rest) = new.split_at(ejected);
    inserted
        .iter()
        .all(|block| matches!(block.kind, BlockKind::Blank))
        && same_tail(old, rest, old_end, new_end)
}

pub(in crate::markdown) fn bounds(
    source: &str,
    siblings: &[Block],
    heading: usize,
    end: usize,
) -> Result<(Range<usize>, usize), StopReason> {
    let block = siblings
        .get(heading)
        .ok_or_else(|| invalid("section heading does not exist"))?;
    let BlockKind::Heading { level, .. } = block.kind else {
        return Err(invalid("selected block is not a heading"));
    };
    let stop = siblings.iter().enumerate().skip(heading+1).find(|(_, next)| matches!(next.kind, BlockKind::Heading { level: next, .. } if next <= level)).map_or(siblings.len(), |(index, _)| index);
    let end = siblings
        .get(stop)
        .map_or(end, |next| line_start(source, next.span.start));
    Ok((block.span.end..end, stop))
}
fn collect(
    doc: &Document,
    blocks: &[Block],
    end: usize,
    path: &mut Vec<usize>,
    result: &mut Vec<SectionSite>,
    g: &mut Governor,
) -> Result<(), StopReason> {
    g.charge(Resource::Depth, size(path.len()))?;
    for (index, block) in blocks.iter().enumerate() {
        g.charge(Resource::Work, 1)?;
        path.push(index);
        if matches!(block.kind, BlockKind::Heading { .. }) {
            let (body, _) = bounds(&doc.source, blocks, index, end)?;
            g.charge(
                Resource::ScratchBytes,
                size(path.len()).saturating_mul(8).saturating_add(128),
            )?;
            result.push(SectionSite {
                source: doc.digest,
                path: path.clone(),
                span: block.span.start..body.end,
                body,
            });
        }
        if !block.children.is_empty() {
            collect(doc, &block.children, block.span.end, path, result, g)?;
        }
        path.pop();
    }
    Ok(())
}
pub(in crate::markdown) fn same_tail(
    old: &[Block],
    new: &[Block],
    old_end: usize,
    new_end: usize,
) -> bool {
    let mut expected = old.to_vec();
    for block in &mut expected {
        shift(block, old_end, new_end);
    }
    expected == new
}
pub(in crate::markdown) fn same_shape(
    left_source: &str,
    left: &[Block],
    right_source: &str,
    right: &[Block],
) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(a, b)| {
            let kind = match (&a.kind, &b.kind) {
                (
                    BlockKind::CodeFence {
                        info: ai,
                        closed: ac,
                        ..
                    },
                    BlockKind::CodeFence {
                        info: bi,
                        closed: bc,
                        ..
                    },
                ) => ai == bi && ac == bc,
                (a, b) => a == b,
            };
            kind && if matches!(a.kind, BlockKind::Blank) {
                true
            } else if a.children.is_empty() && b.children.is_empty() {
                logical_text(left_source, a) == logical_text(right_source, b)
            } else {
                same_shape(left_source, &a.children, right_source, &b.children)
            }
        })
}

pub(in crate::markdown) fn prefix(
    doc: &Document,
    path: &[usize],
    governor: &mut Governor,
) -> Result<String, StopReason> {
    governor.charge(
        Resource::Work,
        size(path.len()).saturating_mul(size(path.len())),
    )?;
    let mut result = String::new();
    for depth in 1..path.len() {
        let block = doc.block_at(&path[..depth])?;
        match block.kind {
            BlockKind::Quote => result.push_str("> "),
            BlockKind::ListItem { .. } => {
                let first = &block.lines[0];
                governor.charge(
                    Resource::Work,
                    size(first.end - line_start(&doc.source, first.start)).saturating_mul(4),
                )?;
                governor.charge(Resource::ScratchBytes, size(first.len()).saturating_mul(4))?;
                let mut column = doc.source[line_start(&doc.source, first.start)..first.start]
                    .chars()
                    .fold(0, |col, ch| {
                        if ch == '\t' {
                            col + 4 - col % 4
                        } else {
                            col + 1
                        }
                    });
                let mut text = " ".repeat(block.line_padding[0]);
                for ch in doc.source[first.clone()].chars() {
                    if ch == '\t' {
                        let width = 4 - column % 4;
                        text.push_str(&" ".repeat(width));
                        column += width;
                    } else {
                        text.push(ch);
                        column += 1;
                    }
                }
                let (_, indent) =
                    crate::markdown::containers::item_prefix(text.trim_end_matches(['\r', '\n']));
                result.push_str(&" ".repeat(indent));
            }
            _ => {
                return Err(invalid("section ancestor is not a container"));
            }
        }
    }
    Ok(result)
}
