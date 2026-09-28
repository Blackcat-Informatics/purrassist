// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Reparsed section-body edits with surrounding block preservation.
mod nested;
use super::{Block, BlockKind, Document, size};
use crate::{
    Budget, Governor, StopReason,
    contract::Resource,
    edits::{EditPlan, TextEdit},
};
pub use nested::SectionSite;

impl Document {
    /// Replace a root section body using the standard resource budget.
    ///
    /// The heading and all blocks outside its section must retain their structure.
    /// Use [`Self::replace_section_governed`] to share an explicit budget.
    ///
    /// # Errors
    /// Rejects missing headings and replacements that change section boundaries
    /// or surrounding blocks. Returns resource exhaustion without a partial plan.
    pub fn replace_section(&self, heading: usize, body: &str) -> Result<EditPlan, StopReason> {
        self.replace_section_governed(heading, body, &mut Governor::new(Budget::default()))
    }

    /// Replace a root section body verbatim, then reparse the complete proposal.
    ///
    /// Lower-ranked subheadings belong to the body and may change. A new heading
    /// of equal or higher rank, an unterminated construct that captures following
    /// blocks, or changed heading spelling is rejected. The caller supplies line
    /// endings and separators; no newline is silently inserted. An identical body
    /// produces an empty plan. Plans support exact conditional undo.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::Document};
    /// let mut g = Governor::new(Budget::default());
    /// let doc = Document::parse("# Tea\nHot.\n# Coffee\nKeep.\n", &mut g)?;
    /// let plan = doc.replace_section_governed(0, "Cold.\n", &mut g)?;
    /// assert_eq!(plan.edits[0].replacement, "Cold.\n");
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects a missing/non-heading root index or a proposal that changes the
    /// selected heading, its ending boundary, or any outside block's source maps
    /// and classification. Cancellation and resource exhaustion propagate.
    pub fn replace_section_governed(
        &self,
        heading: usize,
        body: &str,
        governor: &mut Governor,
    ) -> Result<EditPlan, StopReason> {
        governor.charge(Resource::InputBytes, size(body.len()))?;
        governor.charge(
            Resource::Work,
            size(self.source.len())
                .saturating_mul(8)
                .saturating_add(size(body.len())),
        )?;
        governor.charge(
            Resource::ScratchBytes,
            size(self.source.len())
                .saturating_mul(128)
                .saturating_add(size(body.len()).saturating_mul(4))
                .saturating_add(512),
        )?;
        let section = self
            .sections()
            .into_iter()
            .find(|section| section.heading == heading)
            .ok_or_else(|| invalid("section heading does not exist"))?;
        if self.source[section.body.clone()] == *body {
            return Ok(EditPlan {
                source: self.digest,
                edits: Vec::new(),
            });
        }
        let mut proposed = self.source.clone();
        proposed.replace_range(section.body.clone(), body);
        let updated = Self::parse(&proposed, governor)?;
        let end = section.body.start + body.len();
        let updated_section = updated
            .sections()
            .into_iter()
            .find(|section| section.heading == heading)
            .ok_or_else(|| invalid("replacement removes the selected heading"))?;
        if updated_section.body != (section.body.start..end)
            || self.blocks[..=heading] != updated.blocks[..=heading]
        {
            return Err(invalid(
                "replacement changes the heading or section boundary",
            ));
        }
        let original_tail = self
            .blocks
            .partition_point(|block| block.span.start < section.body.end);
        let updated_tail = updated
            .blocks
            .partition_point(|block| block.span.start < end);
        let mut expected = self.blocks[original_tail..].to_vec();
        for block in &mut expected {
            shift(block, section.body.end, end);
        }
        if expected != updated.blocks[updated_tail..] {
            return Err(invalid("replacement changes surrounding block structure"));
        }
        Ok(EditPlan {
            source: self.digest,
            edits: vec![TextEdit {
                range: section.body,
                replacement: body.into(),
            }],
        })
    }
}

fn shift(block: &mut Block, old: usize, new: usize) {
    let range = |span: &mut std::ops::Range<usize>| {
        span.start = span.start - old + new;
        span.end = span.end - old + new;
    };
    range(&mut block.span);
    for line in &mut block.lines {
        range(line);
    }
    if let BlockKind::CodeFence { content, .. } = &mut block.kind {
        range(content);
    }
    for child in &mut block.children {
        shift(child, old, new);
    }
}
fn invalid(message: &str) -> StopReason {
    StopReason::InvalidInput(message.into())
}
