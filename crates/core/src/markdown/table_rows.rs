// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Source-preserving GFM body-row insertion and removal.
use super::{Block, BlockKind, Document, ending, line, line_start, size};
use crate::contract::invalid_input as invalid;
use crate::{
    Governor, StopReason,
    contract::Resource,
    edits::{EditPlan, TextEdit},
};

/// A structural edit to table body rows, excluding header and delimiter rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableRowEdit {
    /// Insert before the zero-based body index; the current row count appends.
    Insert {
        /// Body insertion position, inclusive of the end.
        index: usize,
        /// Raw inline Markdown, one cell per header column; margins are trimmed.
        cells: Vec<String>,
    },
    /// Remove a body row and its exact container prefix and line ending.
    Remove {
        /// Zero-based body index, excluding header and delimiter rows.
        index: usize,
    },
    /// Reorder a body row, [`Vec::remove`]-then-[`Vec::insert`] semantics.
    Move {
        /// Current zero-based body index.
        index: usize,
        /// Final zero-based body index, `0..rows.len()`.
        to: usize,
    },
}

impl Document {
    /// Insert or remove a GFM body row in a root or nested table.
    ///
    /// Existing bytes outside the edit stay exact. Inserted rows use the delimiter
    /// line's container prefix and newline style, with canonical `| cell |` spacing.
    /// Appending at EOF preserves whether the document has a final newline.
    /// New cells must match the header width; literal pipes must be escaped.
    /// Each proposal is reparsed and its table compared with the expected row edit.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::{Document, TableRowEdit}};
    /// let mut g = Governor::new(Budget::default());
    /// let doc = Document::parse("> | Drink |\n> | --- |\n", &mut g)?;
    /// let edit = TableRowEdit::Insert { index: 0, cells: vec!["iced tea".into()] };
    /// let plan = doc.edit_table_row(&[0, 0], &edit, &mut g)?;
    /// assert_eq!(plan.edits[0].replacement, "> | iced tea |\n");
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects missing/non-table paths, out-of-range rows, wrong cell counts,
    /// line breaks, and cells that change table structure when reparsed.
    /// Budget exhaustion and cancellation propagate without a partial plan.
    pub fn edit_table_row(
        &self,
        path: &[usize],
        edit: &TableRowEdit,
        governor: &mut Governor,
    ) -> Result<EditPlan, StopReason> {
        governor.charge(Resource::Work, size(path.len()).saturating_add(1))?;
        governor.charge(Resource::Depth, size(path.len().saturating_sub(1)))?;
        let block = self.block_at(path)?;
        let BlockKind::Table { header, rows, .. } = &block.kind else {
            return Err(invalid("selected block is not a table"));
        };
        // Reserve table cloning and whole-document proposal construction before allocation.
        governor.charge(
            Resource::ScratchBytes,
            size(self.source.len())
                .saturating_mul(128)
                .saturating_add(512),
        )?;
        governor.charge(Resource::Work, size(self.source.len()).saturating_mul(8))?;
        let mut expected = block.kind.clone();
        let BlockKind::Table {
            rows: expected_rows,
            ..
        } = &mut expected
        else {
            return Err(invalid("selected block is not a table"));
        };
        let change = match edit {
            TableRowEdit::Insert { index, cells } => {
                if *index > rows.len() || cells.len() != header.len() {
                    return Err(invalid("invalid insertion index or cell count"));
                }
                governor.charge(Resource::ScratchBytes, size(cells.len()).saturating_mul(64))?;
                for cell in cells {
                    governor.charge(Resource::InputBytes, size(cell.len()))?;
                    governor.charge(Resource::Work, size(cell.len()).saturating_mul(4))?;
                    governor.charge(Resource::ScratchBytes, size(cell.len()).saturating_mul(4))?;
                    if cell.contains(['\r', '\n']) {
                        return Err(invalid("table cells cannot contain line breaks"));
                    }
                }
                let cells: Vec<String> = cells.iter().map(|cell| cell.trim().into()).collect();
                let change = insert(&self.source, block, *index, &cells)?;
                expected_rows.insert(*index, cells);
                change
            }
            TableRowEdit::Remove { index } => {
                if *index >= rows.len() {
                    return Err(invalid("table body row does not exist"));
                }
                let range = &block.lines[index + 2];
                expected_rows.remove(*index);
                TextEdit {
                    range: line_start(&self.source, range.start)..range.end,
                    replacement: String::new(),
                }
            }
            TableRowEdit::Move { index, to } => {
                if *index >= rows.len() || *to >= rows.len() {
                    return Err(invalid("table row index out of range"));
                }
                if index == to {
                    return Ok(EditPlan {
                        source: self.digest,
                        edits: Vec::new(),
                    });
                }
                let change = move_row(&self.source, block, rows.len(), *index, *to, governor)?;
                let moved_row = expected_rows.remove(*index);
                expected_rows.insert(*to, moved_row);
                change
            }
        };
        let mut proposed = self.source.clone();
        proposed.replace_range(change.range.clone(), &change.replacement);
        let updated = Self::parse(&proposed, governor)?;
        if updated.block_at(path)?.kind != expected {
            return Err(invalid(
                "row edit changes table structure beyond the requested row",
            ));
        }
        Ok(EditPlan {
            source: self.digest,
            edits: vec![change],
        })
    }
}

/// Build the single reordering edit for [`TableRowEdit::Move`]: every body
/// row's raw line, in its new order, each with the delimiter line's own
/// container prefix (every row in one table shares that prefix).
fn move_row(
    source: &str,
    block: &Block,
    row_count: usize,
    index: usize,
    to: usize,
    governor: &mut Governor,
) -> Result<TextEdit, StopReason> {
    let body_lines = &block.lines[2..];
    if body_lines.len() != row_count {
        return Err(invalid("table row source lines missing"));
    }
    let delimiter = &block.lines[1];
    let delimiter_text = line(source, delimiter);
    let indentation = delimiter_text.len() - delimiter_text.trim_start_matches([' ', '\t']).len();
    let row_prefix = &source[line_start(source, delimiter.start)..delimiter.start + indentation];
    let region = line_start(source, body_lines[0].start)..body_lines[row_count - 1].end;
    governor.charge(Resource::Work, size(region.len()).saturating_mul(4))?;
    governor.charge(
        Resource::ScratchBytes,
        size(region.len()).saturating_mul(4).saturating_add(256),
    )?;
    let mut order: Vec<usize> = (0..row_count).collect();
    let moved = order.remove(index);
    order.insert(to, moved);
    let mut replacement = String::with_capacity(region.len());
    for &position in &order {
        replacement.push_str(row_prefix);
        replacement.push_str(&source[body_lines[position].clone()]);
    }
    Ok(TextEdit {
        range: region,
        replacement,
    })
}
fn insert(
    source: &str,
    block: &Block,
    index: usize,
    cells: &[String],
) -> Result<TextEdit, StopReason> {
    let delimiter = block
        .lines
        .get(1)
        .ok_or_else(|| invalid("missing table delimiter source"))?;
    let delimiter_text = line(source, delimiter);
    let indentation = delimiter_text.len() - delimiter_text.trim_start_matches([' ', '\t']).len();
    let prefix = &source[line_start(source, delimiter.start)..delimiter.start + indentation];
    // A delimiter at unterminated EOF has no ending; inherit its header's ending.
    let newline = ending(&source[delimiter.clone()])
        .or_else(|| {
            block
                .lines
                .first()
                .and_then(|range| ending(&source[range.clone()]))
        })
        .unwrap_or("\n");
    let next = block.lines.get(index + 2);
    let position = next.map_or(block.span.end, |range| line_start(source, range.start));
    let at_unterminated_end = position == source.len() && !source.ends_with(['\r', '\n']);
    let mut replacement = String::new();
    if at_unterminated_end {
        replacement.push_str(newline);
    }
    replacement.push_str(prefix);
    replacement.push('|');
    for cell in cells {
        replacement.push(' ');
        replacement.push_str(cell);
        replacement.push_str(" |");
    }
    if !at_unterminated_end {
        replacement.push_str(newline);
    }
    Ok(TextEdit {
        range: position..position,
        replacement,
    })
}
