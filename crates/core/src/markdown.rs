// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Source-preserving Markdown block indexing and section edits.
//!
//! Includes CommonMark/GFM rendering and source-bound structural edit planning.
mod containers;
mod context;
mod extensions;
mod html;
mod lists;
pub use lists::ListItemEdit;
mod table;
mod table_columns;
mod table_rows;
pub use extensions::{Extension, Extensions};
pub use table_columns::TableColumnEdit;
pub use table_rows::TableRowEdit;
mod inline;
mod projection;
mod references;
pub use references::ReferenceSite;
mod render;
mod sections;
pub use context::Context;
pub use inline::{
    Dialect, Inline, InlineFragment, InlineKind, ReferenceDefinition, ReferenceOrigin, References,
};
pub use sections::{SectionInsertion, SectionSite};

use crate::{Governor, StopReason, contract::Resource, digest::Digest};
use std::ops::Range;

/// Recognized source block, retaining the exact original byte range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// Full original range, including original line endings.
    pub span: Range<usize>,
    /// Original line ranges after enclosing container prefixes are removed.
    /// Joining these slices reconstructs this block at its own container level.
    pub lines: Vec<Range<usize>>,
    /// Residual spaces from a partly consumed tab before each line range.
    pub line_padding: Vec<usize>,
    /// Structural classification.
    pub kind: BlockKind,
    /// Nested quote or list-item blocks. Ranges refer to the original document;
    /// the first container prefix is outside each child range. Interior prefixes
    /// remain inside a multi-line child's bounding range.
    pub children: Vec<Self>,
}
/// Editing-oriented block classification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    /// ATX or setext heading; inline spelling is preserved in `title`.
    Heading {
        /// Heading level from one to six.
        level: u8,
        /// Source title, without heading delimiters.
        title: String,
    },
    /// Fenced code; body headings are never indexed as document headings.
    CodeFence {
        /// Opening fence info string.
        info: String,
        /// Code source range excluding fence lines.
        content: Range<usize>,
        /// Whether a matching closing fence exists.
        closed: bool,
    },
    /// Four-space or tab-indented code.
    IndentedCode,
    /// A thematic break.
    ThematicBreak,
    /// A block quote, including any lazy paragraph continuation lines.
    Quote,
    /// A list item with nested body blocks and continuation lines.
    ListItem {
        /// Ordered-list ordinal, absent for bullets.
        ordinal: Option<u64>,
        /// GFM task state when an explicit checkbox exists.
        checked: Option<bool>,
    },
    /// GFM pipe table; raw cells preserve inline spelling and escapes.
    Table {
        /// Header cells.
        header: Vec<String>,
        /// One alignment per header cell.
        alignments: Vec<Alignment>,
        /// Body rows, with their source cells.
        rows: Vec<Vec<String>>,
    },
    /// Link reference definition, kept as source text.
    ReferenceDefinition,
    /// HTML source block, protected from heading edits.
    Html,
    /// Paragraph source.
    Paragraph,
    /// Blank line source.
    Blank,
}
/// GFM table alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    /// No explicit alignment.
    Default,
    /// Leading colon.
    Left,
    /// Trailing colon.
    Right,
    /// Leading and trailing colons.
    Center,
}
/// A heading and its body up to the next heading of equal or higher rank.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// Heading block index; stable only for this document digest.
    pub heading: usize,
    /// Complete section range including its heading and nested subsections.
    pub span: Range<usize>,
    /// Body range excluding the heading.
    pub body: Range<usize>,
}
/// Immutable source and index. Reparse after applying any edit.
#[derive(Clone, Debug)]
pub struct Document {
    source: String,
    digest: Digest,
    blocks: Vec<Block>,
}
impl Document {
    /// Index source blocks while preserving every byte, including CRLF/CR endings.
    ///
    /// # Errors
    /// Returns explicit input, work, storage, or container-depth exhaustion.
    pub fn parse(source: &str, governor: &mut Governor) -> Result<Self, StopReason> {
        governor.charge(Resource::InputBytes, size(source.len()))?;
        governor.charge(Resource::Work, size(source.len()).saturating_mul(4))?;
        governor.charge(
            Resource::ScratchBytes,
            size(source.len()).saturating_mul(128).saturating_add(128),
        )?;
        let blocks = index_blocks(source, governor, true)?;
        Ok(Self {
            source: source.into(),
            digest: Digest::of(source.as_bytes()),
            blocks,
        })
    }
    /// Exact original source.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
    /// Ordered blocks covering the entire source without gaps or overlap.
    #[must_use]
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }
    /// Exact source identity used by every generated edit plan.
    #[must_use]
    pub const fn digest(&self) -> Digest {
        self.digest
    }
    /// Section boundaries; nested lower-ranked headings belong to the parent body.
    #[must_use]
    pub fn sections(&self) -> Vec<Section> {
        let mut sections: Vec<Section> = Vec::new();
        let mut open: Vec<(u8, usize)> = Vec::new();
        for (heading, block) in self.blocks.iter().enumerate() {
            let BlockKind::Heading { level, .. } = block.kind else {
                continue;
            };
            while let Some(&(prior, index)) = open.last() {
                if prior < level {
                    break;
                }
                open.pop();
                sections[index].span.end = block.span.start;
                sections[index].body.end = block.span.start;
            }
            open.push((level, sections.len()));
            sections.push(Section {
                heading,
                span: block.span.start..self.source.len(),
                body: block.span.end..self.source.len(),
            });
        }
        sections
    }
}
fn size(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
fn lines(source: &str) -> Vec<Range<usize>> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut cursor = 0;
    while cursor < source.len() {
        if matches!(source.as_bytes()[cursor], b'\r' | b'\n') {
            let crlf = source.as_bytes()[cursor] == b'\r'
                && source.as_bytes().get(cursor + 1) == Some(&b'\n');
            cursor += if crlf { 2 } else { 1 };
            result.push(start..cursor);
            start = cursor;
        } else {
            cursor += 1;
        }
    }
    if start < source.len() {
        result.push(start..source.len());
    }
    result
}
fn line<'a>(source: &'a str, range: &Range<usize>) -> &'a str {
    source[range.clone()].trim_end_matches(['\r', '\n'])
}
fn unindent(text: &str) -> Option<&str> {
    let count = text.bytes().take_while(|byte| *byte == b' ').count();
    (count <= 3).then(|| &text[count..])
}
fn heading(text: &str) -> Option<(u8, String)> {
    let text = unindent(text)?;
    let count = text.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&count) || !text[count..].starts_with([' ', '\t']) && count != text.len() {
        return None;
    }
    let title = text[count..].trim();
    let suffix = title.trim_end_matches('#');
    let title = if suffix.ends_with([' ', '\t']) || suffix.is_empty() {
        suffix.trim_end()
    } else {
        title
    };
    Some((
        u8::try_from(count).expect("heading at most six"),
        title.into(),
    ))
}
fn fence(text: &str) -> Option<(char, usize, &str)> {
    let text = unindent(text)?;
    let marker = text.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let count = text.chars().take_while(|c| *c == marker).count();
    let info = text[count..].trim();
    (count >= 3 && (marker != '`' || !info.contains('`'))).then_some((marker, count, info))
}
fn setext(text: &str) -> Option<u8> {
    let text = unindent(text)?.trim_end_matches([' ', '\t']);
    if !text.is_empty() && text.bytes().all(|b| b == b'=') {
        Some(1)
    } else if !text.is_empty() && text.bytes().all(|b| b == b'-') {
        Some(2)
    } else {
        None
    }
}
fn thematic(text: &str) -> bool {
    let Some(text) = unindent(text) else {
        return false;
    };
    let chars: Vec<_> = text.chars().filter(|c| !matches!(c, ' ' | '\t')).collect();
    chars.len() >= 3 && matches!(chars[0], '*' | '-' | '_') && chars.iter().all(|c| *c == chars[0])
}
fn code_indent(text: &str) -> bool {
    let mut column = 0;
    for byte in text.bytes() {
        match byte {
            b' ' => column += 1,
            b'\t' => column += 4 - column % 4,
            _ => return false,
        }
        if column >= 4 {
            return true;
        }
    }
    false
}
fn index_blocks(
    source: &str,
    governor: &mut Governor,
    tables: bool,
) -> Result<Vec<Block>, StopReason> {
    let mut blocks = if source.contains('\t') {
        let projection = projection::Projection::new(source);
        let lines = lines(&projection.text);
        let mut blocks = containers::index(&projection.text, &lines, governor, 0, tables)?;
        projection.restore(source, &mut blocks);
        blocks
    } else {
        let lines = lines(source);
        containers::index(source, &lines, governor, 0, tables)?
    };
    classify_tasks(source, &mut blocks, governor)?;
    Ok(blocks)
}
fn classify_tasks(
    source: &str,
    blocks: &mut [Block],
    governor: &mut Governor,
) -> Result<(), StopReason> {
    for block in blocks {
        governor.charge(Resource::Work, 1)?;
        classify_tasks(source, &mut block.children, governor)?;
        if !matches!(block.kind, BlockKind::ListItem { .. }) {
            continue;
        }
        let checked = if let Some(paragraph) = block
            .children
            .iter()
            .find(|child| {
                !matches!(
                    child.kind,
                    BlockKind::Blank | BlockKind::ReferenceDefinition
                )
            })
            .filter(|child| matches!(child.kind, BlockKind::Paragraph))
        {
            governor.charge(Resource::Work, size(paragraph.span.len()))?;
            governor.charge(
                Resource::ScratchBytes,
                size(paragraph.span.len()).saturating_mul(2),
            )?;
            let text = render::paragraph_text(source, paragraph, Some(block));
            task_marker(text.trim_end_matches(['\r', '\n'])).map(|(state, _)| state)
        } else {
            None
        };
        if let BlockKind::ListItem { checked: state, .. } = &mut block.kind {
            *state = checked;
        }
    }
    Ok(())
}
fn classify(
    source: &str,
    lines: &[Range<usize>],
    cursor: usize,
    tables: bool,
    lazy: &[usize],
) -> (BlockKind, usize) {
    let text = line(source, &lines[cursor]);
    if text.trim_matches([' ', '\t']).is_empty() {
        return (BlockKind::Blank, cursor + 1);
    }
    if let Some((marker, count, info)) = fence(text) {
        let mut end = cursor + 1;
        while end < lines.len() {
            if let Some((other, width, rest)) = fence(line(source, &lines[end]))
                && marker == other
                && width >= count
                && rest.is_empty()
            {
                break;
            }
            end += 1;
        }
        let content_end = lines.get(end).map_or_else(
            || lines.last().map_or(0, |range| range.end),
            |range| range.start,
        );
        return (
            BlockKind::CodeFence {
                info: info.into(),
                content: lines[cursor].end..content_end,
                closed: end < lines.len(),
            },
            (end + 1).min(lines.len()),
        );
    }
    if let Some((level, title)) = heading(text) {
        return (BlockKind::Heading { level, title }, cursor + 1);
    }
    if thematic(text) {
        return (BlockKind::ThematicBreak, cursor + 1);
    }
    if code_indent(text) {
        let mut end = cursor + 1;
        while end < lines.len() && {
            let next = line(source, &lines[end]);
            code_indent(next) || next.trim_matches([' ', '\t']).is_empty()
        } {
            end += 1;
        }
        return (BlockKind::IndentedCode, end);
    }
    if let Some(ending) = html::start(text, false) {
        let mut end = cursor + 1;
        if !ending.closes(text) {
            while end < lines.len() {
                let done = ending.closes(line(source, &lines[end]));
                if done && matches!(ending, html::End::Blank) {
                    break;
                }
                end += 1;
                if done {
                    break;
                }
            }
        }
        return (BlockKind::Html, end);
    }
    if unindent(text).is_some_and(|text| text.starts_with('>')) {
        return (BlockKind::Quote, cursor + 1);
    }
    if let Some(kind) = list_item(text) {
        return (kind, cursor + 1);
    }
    paragraph(source, lines, cursor, tables, lazy)
}
fn paragraph(
    source: &str,
    lines: &[Range<usize>],
    cursor: usize,
    tables: bool,
    lazy: &[usize],
) -> (BlockKind, usize) {
    let text = line(source, &lines[cursor]);
    let mut table_rejected = false;
    if cursor + 1 < lines.len() {
        let next = line(source, &lines[cursor + 1]);
        if let Some(level) = setext(next) {
            return (
                BlockKind::Heading {
                    level,
                    title: text.trim().into(),
                },
                cursor + 2,
            );
        }
        let header = cells(text);
        if tables
            && lazy.binary_search(&lines[cursor + 1].start).is_err()
            && !interrupts_paragraph(next)
            && let Some(alignments) = alignments(next)
            && !header.is_empty()
            && header.len() == alignments.len()
        {
            let mut end = cursor + 2;
            let mut rows = Vec::new();
            while end < lines.len()
                && !interrupts_paragraph(line(source, &lines[end]))
                && list_item(line(source, &lines[end])).is_none()
                && !code_indent(line(source, &lines[end]))
                && html::start(line(source, &lines[end]), false).is_none()
            {
                let row = cells(line(source, &lines[end]));
                if row.is_empty() {
                    break;
                }
                rows.push(row);
                end += 1;
            }
            return (
                BlockKind::Table {
                    header,
                    alignments,
                    rows,
                },
                end,
            );
        }
    }
    let mut end = cursor + 1;
    while end < lines.len() {
        let next = line(source, &lines[end]);
        if let Some(level) = setext(next) {
            let title = lines[cursor..end]
                .iter()
                .map(|range| line(source, range).trim())
                .collect::<Vec<_>>()
                .join("\n");
            return (BlockKind::Heading { level, title }, end + 1);
        }
        // A delimiter candidate whose width disagrees with its header keeps
        // this paragraph open. Later delimiter-looking lines cannot restart
        // table recognition until a genuine block boundary closes it.
        if tables && lazy.binary_search(&lines[end].start).is_err() && alignments(next).is_some() {
            table_rejected = true;
        }
        if interrupts_paragraph(next)
            || (tables
                && !table_rejected
                && end + 1 < lines.len()
                && !code_indent(next)
                && lazy.binary_search(&lines[end + 1].start).is_err()
                && !interrupts_paragraph(line(source, &lines[end + 1]))
                && setext(line(source, &lines[end + 1])).is_none()
                && alignments(line(source, &lines[end + 1])).is_some_and(|alignments| {
                    let header = cells(next);
                    !header.is_empty() && header.len() == alignments.len()
                }))
        {
            break;
        }
        end += 1;
    }
    (BlockKind::Paragraph, end)
}
fn interrupts_paragraph(text: &str) -> bool {
    text.trim_matches([' ', '\t']).is_empty()
        || heading(text).is_some()
        || fence(text).is_some()
        || thematic(text)
        || html::start(text, true).is_some()
        || unindent(text).is_some_and(|s| s.starts_with('>'))
        || list_item(text).is_some_and(|item| {
            let BlockKind::ListItem { ordinal, .. } = item else {
                return false;
            };
            let rest = text.trim_start().trim_start_matches(|c: char| {
                c.is_ascii_digit() || matches!(c, '.' | ')' | '-' | '+' | '*')
            });
            (ordinal.is_none() || ordinal == Some(1)) && !rest.trim().is_empty()
        })
}
fn list_item(text: &str) -> Option<BlockKind> {
    let text = unindent(text)?;
    let (ordinal, rest) = if matches!(text.as_bytes().first(), Some(b'-' | b'+' | b'*')) {
        (None, &text[1..])
    } else {
        let width = text.bytes().take_while(u8::is_ascii_digit).count();
        if !(1..=9).contains(&width) || !matches!(text.as_bytes().get(width), Some(b'.' | b')')) {
            return None;
        }
        (text[..width].parse().ok(), &text[width + 1..])
    };
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    let rest = rest.trim_start_matches([' ', '\t']);
    let checked = task_marker(rest).map(|(checked, _)| checked);
    Some(BlockKind::ListItem { ordinal, checked })
}
// Task processing follows paragraph parsing. CRLF represents one logical
// newline, and a marker needs whitespace separating it from later content.
fn task_marker(text: &str) -> Option<(bool, usize)> {
    let marker = text.trim_start_matches(' ');
    let leading = text.len() - marker.len();
    let rest = marker.strip_prefix('[')?;
    let middle = rest.chars().next()?;
    let checked = match middle {
        'x' | 'X' => true,
        ' ' | '\t' | '\r' | '\n' | '\u{b}' | '\u{c}' => false,
        _ => return None,
    };
    let width = if rest.starts_with("\r\n") {
        2
    } else {
        middle.len_utf8()
    };
    let tail = rest.get(width..)?.strip_prefix(']')?;
    if !tail.starts_with([' ', '\t', '\r', '\n', '\u{b}', '\u{c}']) {
        return None;
    }
    Some((checked, leading + width + 2))
}

fn cells(text: &str) -> Vec<String> {
    cell_ranges(text)
        .into_iter()
        .map(|range| text[range].trim().into())
        .collect()
}
fn cell_ranges(text: &str) -> Vec<Range<usize>> {
    let offset = text.len() - text.trim_start().len();
    let text = text.trim();
    let mut result = Vec::new();
    let mut start = 0;
    let mut escaped = false;
    for (position, character) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
        } else if character == '|' {
            result.push(offset + start..offset + position);
            start = position + 1;
        }
    }
    result.push(offset + start..offset + text.len());
    if start == text.len() && start > 0 {
        result.pop();
    }
    if text.starts_with('|') {
        result.remove(0);
    }
    result
}
fn alignments(text: &str) -> Option<Vec<Alignment>> {
    let text = unindent(text)?;
    let cells = cells(text);
    if cells.is_empty() {
        return None;
    }
    cells
        .iter()
        .map(|cell| {
            let core = cell.trim_matches(':');
            if core.is_empty() || !core.bytes().all(|byte| byte == b'-') {
                return None;
            }
            Some(match (cell.starts_with(':'), cell.ends_with(':')) {
                (true, true) => Alignment::Center,
                (true, false) => Alignment::Left,
                (false, true) => Alignment::Right,
                _ => Alignment::Default,
            })
        })
        .collect()
}
