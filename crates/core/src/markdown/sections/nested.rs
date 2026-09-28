// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Section paths and container-aware body insertion.
use super::{Block, BlockKind, Document, invalid, shift, size};
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
        if logical == body {
            return Ok(EditPlan {
                source: self.digest,
                edits: Vec::new(),
            });
        }
        let prefix = prefix(self, path, governor)?;
        let lines = crate::markdown::lines(body);
        governor.charge(
            Resource::Work,
            size(prefix.len()).saturating_mul(size(lines.len())),
        )?;
        governor.charge(
            Resource::ScratchBytes,
            size(prefix.len())
                .saturating_mul(size(lines.len()))
                .saturating_mul(4),
        )?;
        let mut replacement = String::new();
        for line in lines {
            replacement.push_str(&prefix);
            replacement.push_str(&body[line]);
        }
        let mut proposed = self.source.clone();
        proposed.replace_range(range.clone(), &replacement);
        let updated = Self::parse(&proposed, governor)?;
        let new_end = range.start + replacement.len();
        let new_parent = updated.block_at(&path[..path.len() - 1])?;
        let (new_range, new_stop) = bounds(
            &proposed,
            &new_parent.children,
            heading,
            new_parent.span.end,
        )?;
        if new_range != (range.start..new_end) {
            return Err(invalid("replacement changes nested section boundaries"));
        }
        let intended = Self::parse(body, governor)?;
        if !same_shape(
            &intended.source,
            &intended.blocks,
            &updated.source,
            &new_parent.children[heading + 1..new_stop],
        ) {
            return Err(invalid(
                "replacement changes body structure inside its container",
            ));
        }
        validate_context(self, &updated, path, stop, new_stop, &range, new_end)?;
        Ok(EditPlan {
            source: self.digest,
            edits: vec![TextEdit { range, replacement }],
        })
    }
}

fn validate_context(
    original: &Document,
    updated: &Document,
    path: &[usize],
    stop: usize,
    new_stop: usize,
    range: &Range<usize>,
    new_end: usize,
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

fn bounds(
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
fn same_tail(old: &[Block], new: &[Block], old_end: usize, new_end: usize) -> bool {
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
fn logical_text(source: &str, block: &Block) -> String {
    let mut text = String::new();
    for (line, padding) in block.lines.iter().zip(&block.line_padding) {
        text.extend(std::iter::repeat_n(' ', *padding));
        text.push_str(&source[line.clone()]);
    }
    text
}

fn line_start(source: &str, offset: usize) -> usize {
    source[..offset]
        .rfind(['\r', '\n'])
        .map_or(0, |position| position + 1)
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
            _ => return Err(invalid("section ancestor is not a container")),
        }
    }
    Ok(result)
}
