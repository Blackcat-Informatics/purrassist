// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Governed table-column changes retaining untouched cell spelling and source lines.
use super::{Alignment, BlockKind, Document, cell_ranges, line, size};
use crate::{
    Governor, Outcome, StopReason,
    contract::Resource,
    edits::{EditPlan, TextEdit},
};

/// A column-wide structural edit; indices refer to the original header width.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableColumnEdit {
    /// Insert a header, alignment, and one supplied cell per body row.
    Insert {
        /// Insert before this index; the header width appends.
        index: usize,
        /// Raw inline Markdown for the new header.
        header: String,
        /// New column alignment.
        alignment: Alignment,
        /// Raw inline Markdown, exactly one value per body row.
        cells: Vec<String>,
    },
    /// Remove a column from the header, delimiter, and all body rows.
    Remove {
        /// Existing header column; the last remaining column cannot be removed.
        index: usize,
    },
    /// Change only an alignment delimiter's content, preserving its margins.
    Align {
        /// Existing header column.
        index: usize,
        /// Requested alignment.
        alignment: Alignment,
    },
}

impl Document {
    /// Plan a column insertion, removal, or alignment change in a nested GFM table.
    ///
    /// Existing cell spelling, container prefixes, line endings, and outer margins
    /// remain exact. Insert/remove adds outer pipes when absent and materializes
    /// implicit empty body cells before shifting columns. Ignored extra cells remain
    /// beyond the header width. New cell margins are trimmed; literal pipes require
    /// escaping. The full proposal is reparsed against the expected table.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::{Document, TableColumnEdit, Alignment}};
    /// let mut g = Governor::new(Budget::default());
    /// let doc = Document::parse("| A | B |\n| --- | --- |\n| tea | cold |\n", &mut g)?;
    /// let plan = doc.edit_table_column(&[0], &TableColumnEdit::Align {
    ///     index: 1, alignment: Alignment::Right,
    /// }, &mut g)?;
    /// assert_eq!(plan.edits.len(), 1);
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects invalid paths/indices, removing the final column, wrong body-cell
    /// counts, line breaks, and changes to an unselected cell's parsed content.
    /// Resource exhaustion and cancellation return no partial plan. Padding growth
    /// is admitted before allocation, including sparse tables with wide headers.
    pub fn edit_table_column(
        &self,
        path: &[usize],
        edit: &TableColumnEdit,
        governor: &mut Governor,
    ) -> Result<EditPlan, StopReason> {
        governor.charge(Resource::Work, size(path.len()).saturating_add(1))?;
        governor.charge(Resource::Depth, size(path.len().saturating_sub(1)))?;
        let block = self.block_at(path)?;
        let BlockKind::Table {
            header,
            rows,
            alignments,
        } = &block.kind
        else {
            return Err(invalid("selected block is not a table"));
        };
        admit(edit, header.len(), rows.len(), governor)?;
        let mut plan = EditPlan {
            source: self.digest,
            edits: Vec::new(),
        };
        if let TableColumnEdit::Align { index, alignment } = edit
            && alignments[*index] == *alignment
        {
            return Ok(plan);
        }
        let expanded = if matches!(edit, TableColumnEdit::Align { .. }) {
            0
        } else {
            size(header.len())
                .saturating_add(1)
                .saturating_mul(size(rows.len()).saturating_add(2))
        };
        governor.charge(
            Resource::ScratchBytes,
            size(self.source.len())
                .saturating_mul(128)
                .saturating_add(expanded.saturating_mul(128)),
        )?;
        governor.charge(
            Resource::Work,
            size(self.source.len())
                .saturating_mul(8)
                .saturating_add(expanded.saturating_mul(8)),
        )?;
        let mut expected = block.kind.clone();
        update_expected(&mut expected, edit);
        for (number, range) in block.lines.iter().enumerate() {
            if matches!(edit, TableColumnEdit::Align { .. }) && number != 1 {
                continue;
            }
            let text = line(&self.source, range);
            let replacement = edit_line(text, header.len(), number, edit);
            if replacement != text {
                plan.edits.push(TextEdit {
                    range: range.start..range.start + text.len(),
                    replacement,
                });
            }
        }
        let applied = plan.apply_governed(&self.source, governor);
        let Outcome::Resolved(applied_value) = applied.outcome else {
            return Err(applied.receipt.stopped);
        };
        let updated = Self::parse(&applied_value.text, governor)?;
        if updated.block_at(path)?.kind != expected {
            return Err(invalid(
                "column edit changes unselected table content or structure",
            ));
        }
        Ok(plan)
    }
}

fn admit(
    edit: &TableColumnEdit,
    width: usize,
    rows: usize,
    g: &mut Governor,
) -> Result<(), StopReason> {
    match edit {
        TableColumnEdit::Insert {
            index,
            header,
            cells,
            ..
        } => {
            if *index > width || cells.len() != rows {
                return Err(invalid("invalid column index or body-cell count"));
            }
            for value in std::iter::once(header).chain(cells) {
                g.charge(Resource::InputBytes, size(value.len()))?;
                g.charge(
                    Resource::Work,
                    size(value.len()).saturating_mul(8).saturating_add(1),
                )?;
                g.charge(
                    Resource::ScratchBytes,
                    size(value.len()).saturating_mul(8).saturating_add(128),
                )?;
                if value.contains(['\r', '\n']) {
                    return Err(invalid("table cells cannot contain line breaks"));
                }
            }
        }
        TableColumnEdit::Remove { index } => {
            if *index >= width || width == 1 {
                return Err(invalid("cannot remove missing or final table column"));
            }
        }
        TableColumnEdit::Align { index, .. } => {
            if *index >= width {
                return Err(invalid("table column does not exist"));
            }
        }
    }
    Ok(())
}
fn update_expected(kind: &mut BlockKind, edit: &TableColumnEdit) {
    if let BlockKind::Table {
        header,
        rows,
        alignments,
    } = kind
    {
        if !matches!(edit, TableColumnEdit::Align { .. }) {
            for row in rows.iter_mut() {
                if row.len() < header.len() {
                    row.resize(header.len(), String::new());
                }
            }
        }
        match edit {
            TableColumnEdit::Insert {
                index,
                header: name,
                alignment,
                cells,
            } => {
                header.insert(*index, name.trim().into());
                alignments.insert(*index, *alignment);
                for (row, value) in rows.iter_mut().zip(cells) {
                    row.insert(*index, value.trim().into());
                }
            }
            TableColumnEdit::Remove { index } => {
                header.remove(*index);
                alignments.remove(*index);
                for row in rows {
                    row.remove(*index);
                }
            }
            TableColumnEdit::Align { index, alignment } => {
                alignments[*index] = *alignment;
            }
        }
    }
}
fn edit_line(text: &str, width: usize, number: usize, edit: &TableColumnEdit) -> String {
    let ranges = cell_ranges(text);
    if let TableColumnEdit::Align { index, alignment } = edit {
        let raw = &ranges[*index];
        let value = &text[raw.clone()];
        let start = raw.start + value.len() - value.trim_start().len();
        let mut result = text.to_owned();
        result.replace_range(start..start + value.trim().len(), delimiter(*alignment));
        return result;
    }
    let mut cells: Vec<String> = ranges
        .iter()
        .map(|range| text[range.clone()].into())
        .collect();
    if cells.len() < width {
        cells.resize(width, String::new());
    }
    match edit {
        TableColumnEdit::Insert {
            index,
            header,
            alignment,
            cells: values,
        } => {
            let value = match number {
                0 => header,
                1 => delimiter(*alignment),
                _ => &values[number - 2],
            };
            cells.insert(*index, format!(" {} ", value.trim()));
        }
        TableColumnEdit::Remove { index } => {
            cells.remove(*index);
        }
        TableColumnEdit::Align { .. } => unreachable!(),
    }
    let leading = text.len() - text.trim_start().len();
    let prefix = ranges
        .first()
        .map_or(&text[..leading], |range| &text[..range.start]);
    let suffix = ranges.last().map_or_else(
        || &text[text.trim_end().len()..],
        |range| &text[range.end..],
    );
    let mut result = prefix.to_owned();
    if !prefix.trim_end().ends_with('|') {
        result.push('|');
    }
    result.push_str(&cells.join("|"));
    if !suffix.trim_start().starts_with('|') {
        result.push('|');
    }
    result.push_str(suffix);
    result
}
const fn delimiter(alignment: Alignment) -> &'static str {
    match alignment {
        Alignment::Default => "---",
        Alignment::Left => ":---",
        Alignment::Right => "---:",
        Alignment::Center => ":---:",
    }
}
fn invalid(message: &str) -> StopReason {
    StopReason::InvalidInput(message.into())
}
