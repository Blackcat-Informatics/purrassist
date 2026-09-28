// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! GFM cell edits preserve source delimiters and validate the resulting table.
use super::{BlockKind, Document, cell_ranges, line, size};
use crate::{
    Governor, StopReason,
    contract::Resource,
    edits::{EditPlan, TextEdit},
};

impl Document {
    /// Replace one explicitly present GFM table cell with raw inline Markdown.
    ///
    /// `path` selects a table through root/child indices. Row zero is the header;
    /// row one is the first body row (the alignment delimiter is never a data row).
    /// Replacement margins are trimmed. Existing cell margins, pipes, container
    /// prefixes and line endings stay byte-exact.
    /// The proposal is reparsed to verify the selected table's full cell content
    /// and alignment. No-op replacements return an empty source-bound plan.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::Document};
    /// let mut g = Governor::new(Budget::default());
    /// let doc = Document::parse("| drink |\n| --- |\n| tea |\n", &mut g)?;
    /// let plan = doc.replace_table_cell(&[0], 1, 0, "**iced tea**", &mut g)?;
    /// assert_eq!(plan.edits[0].replacement, "**iced tea**");
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects missing/non-table paths, absent cells (including implicit padded
    /// cells), cells beyond the header width, line breaks, and proposals that
    /// alter table structure or another cell. Escape literal pipes as `\|`.
    /// Work, storage, depth exhaustion and cancellation propagate explicitly.
    pub fn replace_table_cell(
        &self,
        path: &[usize],
        row: usize,
        column: usize,
        markdown: &str,
        governor: &mut Governor,
    ) -> Result<EditPlan, StopReason> {
        governor.charge(
            Resource::Work,
            size(path.len()).saturating_add(size(markdown.len())),
        )?;
        governor.charge(Resource::Depth, size(path.len().saturating_sub(1)))?;
        governor.charge(Resource::InputBytes, size(markdown.len()))?;
        if markdown.contains(['\r', '\n']) {
            return Err(invalid("table cells cannot contain line breaks"));
        }
        let block = self.block_at(path)?;
        let BlockKind::Table { header, rows, .. } = &block.kind else {
            return Err(invalid("selected block is not a table"));
        };
        let cells = if row == 0 {
            header
        } else {
            rows.get(row - 1)
                .ok_or_else(|| invalid("table row does not exist"))?
        };
        if column >= header.len() || column >= cells.len() {
            return Err(invalid(
                "table cell is not explicitly present within header width",
            ));
        }
        let line_index = if row == 0 {
            0
        } else {
            row.checked_add(1)
                .ok_or_else(|| invalid("table row exceeds address space"))?
        };
        let source_line = block
            .lines
            .get(line_index)
            .ok_or_else(|| invalid("table source line missing"))?;
        let text = line(&self.source, source_line);
        governor.charge(Resource::Work, size(text.len()).saturating_mul(4))?;
        governor.charge(
            Resource::ScratchBytes,
            size(text.len()).saturating_mul(32).saturating_add(256),
        )?;
        let ranges = cell_ranges(text);
        let raw = ranges
            .get(column)
            .ok_or_else(|| invalid("table source cell missing"))?;
        let value = &text[raw.clone()];
        let start = raw.start + value.len() - value.trim_start().len();
        let end = start + value.trim().len();
        let replacement = markdown.trim();
        let mut plan = EditPlan {
            source: self.digest,
            edits: Vec::new(),
        };
        if &text[start..end] == replacement {
            return Ok(plan);
        }
        // Admission precedes cloning or constructing the complete proposed document.
        let bytes = size(self.source.len()).saturating_add(size(replacement.len()));
        governor.charge(
            Resource::ScratchBytes,
            bytes.saturating_mul(4).saturating_add(256),
        )?;
        governor.charge(Resource::Work, bytes.saturating_mul(4))?;
        let range = source_line.start + start..source_line.start + end;
        let mut proposed = self.source.clone();
        proposed.replace_range(range.clone(), replacement);
        let updated = Self::parse(&proposed, governor)?;
        let mut expected = block.kind.clone();
        if let BlockKind::Table { header, rows, .. } = &mut expected {
            if row == 0 {
                header[column] = replacement.into();
            } else {
                rows[row - 1][column] = replacement.into();
            }
        }
        if updated.block_at(path)?.kind != expected {
            return Err(invalid(
                "replacement changes table structure or another cell",
            ));
        }
        plan.edits.push(TextEdit {
            range,
            replacement: replacement.into(),
        });
        Ok(plan)
    }
}
fn invalid(message: &str) -> StopReason {
    StopReason::InvalidInput(message.into())
}
