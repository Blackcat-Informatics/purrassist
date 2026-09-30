// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Source-bound context and task editing without implicit source normalization.
use super::{Block, BlockKind, Document, Section, size};
use crate::{
    Governor, StopReason,
    contract::Resource,
    digest::Digest,
    edits::{EditPlan, TextEdit},
};
use std::ops::Range;

/// An exact source excerpt expanded to complete indexed block boundaries.
///
/// `span` locates `text` in the original document; `focus` is the caller's original
/// selection, not an offset into the excerpt. Headings are ordered outermost first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Context {
    /// Identity binding this excerpt to the complete source document.
    pub source: Digest,
    /// Caller-selected range in the complete source.
    pub focus: Range<usize>,
    /// Complete excerpt range in the source.
    pub span: Range<usize>,
    /// Exact source bytes, including whitespace and line endings.
    pub text: String,
    /// Sections containing the entire focus, from outer to inner.
    pub sections: Vec<Section>,
}
impl Document {
    /// Expand a selection to whole indexed blocks and retain neighboring blocks.
    ///
    /// Empty selections select the block containing the cursor; EOF selects the
    /// final block. `neighbors` counts indexed blocks on each side, including blank
    /// blocks. No content is silently truncated when a budget is insufficient.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::Document};
    /// let mut governor = Governor::new(Budget::default());
    /// let document = Document::parse("# Tea\nServe cold.\n", &mut governor).unwrap();
    /// let context = document.context(7..11, 0, &mut governor).unwrap();
    /// assert_eq!(context.text, "Serve cold.\n");
    /// assert_eq!(context.sections.len(), 1);
    /// ```
    ///
    /// # Errors
    /// Rejects reversed, out-of-bounds, or non-UTF-8-boundary ranges; returns sticky
    /// work/storage exhaustion before copying the excerpt or allocating sections.
    pub fn context(
        &self,
        focus: Range<usize>,
        neighbors: usize,
        governor: &mut Governor,
    ) -> Result<Context, StopReason> {
        governor.check()?;
        if focus.start > focus.end || self.source.get(focus.clone()).is_none() {
            return Err(StopReason::InvalidInput(
                "context range is not a valid source range".into(),
            ));
        }
        governor.charge(Resource::Work, size(self.blocks.len()).saturating_mul(4))?;
        let (first, last) = if focus.is_empty() {
            let index = self
                .blocks
                .partition_point(|block| block.span.end <= focus.start)
                .min(self.blocks.len().saturating_sub(1));
            (index, index)
        } else {
            (
                self.blocks
                    .partition_point(|block| block.span.end <= focus.start),
                self.blocks
                    .partition_point(|block| block.span.start < focus.end)
                    .saturating_sub(1),
            )
        };
        let first = first.saturating_sub(neighbors);
        let last = last
            .saturating_add(neighbors)
            .min(self.blocks.len().saturating_sub(1));
        let span = self.blocks.get(first).map_or(0, |block| block.span.start)
            ..self.blocks.get(last).map_or(0, |block| block.span.end);
        governor.charge(
            Resource::ScratchBytes,
            size(span.len()).saturating_add(size(self.blocks.len()).saturating_mul(256)),
        )?;
        governor.charge(Resource::Work, size(span.len()))?;
        let sections = self
            .sections()
            .into_iter()
            .filter(|section| {
                section.span.start <= focus.start
                    && focus.end <= section.span.end
                    && (focus.start < section.span.end || focus.start == self.source.len())
            })
            .collect();
        Ok(Context {
            source: self.digest,
            focus,
            text: self.source[span.clone()].into(),
            span,
            sections,
        })
    }

    /// Change only the checkbox state of a recognized GFM task item.
    ///
    /// The returned plan is bound to the complete source digest and supports the
    /// normal edit engine's exact undo. Already-matching states yield an empty plan.
    /// Recognition follows the first parsed paragraph; content may begin on its
    /// next line. A split marker's whitespace is replaced by joining its marker
    /// lines, with exact original spelling retained by the inverse edit.
    /// Optional leading ASCII spaces are part of marker admission; the edit
    /// changes only its state and retains those original spaces.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, Outcome, markdown::Document};
    /// let source = "- [x]\n  tea\n";
    /// let doc = Document::parse(source, &mut Governor::new(Budget::default()))?;
    /// let plan = doc.set_task(0, false)?;
    /// let Outcome::Resolved(changed) = plan.apply(source, Budget::default()).outcome else {
    ///     panic!("the source-bound task edit must apply");
    /// };
    /// assert_eq!(changed.text, "- [ ]\n  tea\n");
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, Outcome, markdown::Document};
    /// let source = "- [ref]: /tea\n [ ] tea\n";
    /// let doc = Document::parse(source, &mut Governor::new(Budget::default()))?;
    /// let plan = doc.set_task(0, true)?;
    /// let Outcome::Resolved(changed) = plan.apply(source, Budget::default()).outcome else {
    ///     panic!("the original task marker must remain editable");
    /// };
    /// assert_eq!(changed.text, "- [ref]: /tea\n [x] tea\n");
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects missing block indices and blocks that are not recognized task items.
    pub fn set_task(&self, block: usize, checked: bool) -> Result<EditPlan, StopReason> {
        self.task_plan(self.block_at(&[block])?, checked)
    }

    /// Select a block by root index followed by successive child indices.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::{Document, BlockKind}};
    /// let doc = Document::parse("> - [ ] tea\n", &mut Governor::new(Budget::default()))?;
    /// assert!(matches!(doc.block_at(&[0, 0])?.kind, BlockKind::ListItem { .. }));
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects empty paths or any index outside its selected parent's children.
    pub fn block_at(&self, path: &[usize]) -> Result<&Block, StopReason> {
        let (first, rest) = path
            .split_first()
            .ok_or_else(|| StopReason::InvalidInput("block path is empty".into()))?;
        let missing = || StopReason::InvalidInput("block path does not exist".into());
        let mut block = self.blocks.get(*first).ok_or_else(missing)?;
        for index in rest {
            block = block.children.get(*index).ok_or_else(missing)?;
        }
        Ok(block)
    }

    /// Plan a checkbox edit anywhere in a nested quote/list block tree.
    ///
    /// The path is relative to this document's immutable block index. Only the
    /// marker state changes. A marker whose whitespace is a logical newline
    /// joins its split marker lines; that explicit splice includes intervening
    /// container indentation. All bytes outside the splice stay exact, and undo
    /// restores the original line endings and prefixes.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::Document};
    /// let mut g = Governor::new(Budget::default());
    /// let doc = Document::parse("> - [ ] tea\r\n", &mut g)?;
    /// let plan = doc.set_task_at(&[0, 0], true, &mut g)?;
    /// assert_eq!(plan.edits[0].range, 5..6);
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects missing/non-task selections; returns resource exhaustion or cancellation.
    /// Already-matching states produce an empty plan bound to the same source digest.
    pub fn set_task_at(
        &self,
        path: &[usize],
        checked: bool,
        governor: &mut Governor,
    ) -> Result<EditPlan, StopReason> {
        governor.charge(Resource::Work, size(path.len()).saturating_add(1))?;
        governor.charge(Resource::Depth, size(path.len().saturating_sub(1)))?;
        governor.charge(Resource::ScratchBytes, 256)?;
        self.task_plan(self.block_at(path)?, checked)
    }

    fn task_plan(&self, block: &Block, checked: bool) -> Result<EditPlan, StopReason> {
        let BlockKind::ListItem {
            checked: Some(current),
            ..
        } = block.kind
        else {
            return Err(StopReason::InvalidInput("block is not a task item".into()));
        };
        let mut edits = Vec::new();
        if current != checked {
            let paragraph = block
                .children
                .iter()
                .find(|child| {
                    !matches!(
                        child.kind,
                        BlockKind::ReferenceDefinition | BlockKind::Blank
                    )
                })
                .ok_or_else(|| StopReason::InvalidInput("task paragraph is missing".into()))?;
            let first = paragraph.lines.first().ok_or_else(|| {
                StopReason::InvalidInput("task paragraph has no source line".into())
            })?;
            let position = self.source[first.clone()]
                .find('[')
                .ok_or_else(|| StopReason::InvalidInput("task marker is missing".into()))?
                + first.start
                + 1;
            let end = if self.source[position..].starts_with(['\r', '\n']) {
                let closing = paragraph.lines.get(1).ok_or_else(|| {
                    StopReason::InvalidInput("split task marker has no closing line".into())
                })?;
                closing.start + self.source[closing.clone()].len()
                    - self.source[closing.clone()]
                        .trim_start_matches([' ', '\t'])
                        .len()
            } else {
                position + 1
            };
            edits.push(TextEdit {
                range: position..end,
                replacement: if checked { "x" } else { " " }.into(),
            });
        }
        Ok(EditPlan {
            source: self.digest,
            edits,
        })
    }
}
