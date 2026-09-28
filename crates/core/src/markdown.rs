// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Source-preserving Markdown block indexing and section edits.
//!
//! This is an editing index, not yet a conforming CommonMark/GFM renderer.
use crate::{
    Governor, StopReason,
    contract::Resource,
    digest::Digest,
    edits::{EditPlan, TextEdit},
};
use std::ops::Range;

/// Recognized source block, retaining the exact original byte range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// Full original range, including original line endings.
    pub span: Range<usize>,
    /// Structural classification.
    pub kind: BlockKind,
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
    /// Consecutive quote-prefixed source lines.
    Quote,
    /// A list item start; continuation lines retain their own source blocks.
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
    /// Returns explicit input, work, or storage exhaustion.
    pub fn parse(source: &str, governor: &mut Governor) -> Result<Self, StopReason> {
        governor.charge(Resource::InputBytes, size(source.len()))?;
        governor.charge(Resource::Work, size(source.len()).saturating_mul(4))?;
        governor.charge(
            Resource::ScratchBytes,
            size(source.len()).saturating_mul(128).saturating_add(128),
        )?;
        let lines = lines(source);
        let mut blocks = Vec::new();
        let mut cursor = 0;
        while cursor < lines.len() {
            let (kind, end) = classify(source, &lines, cursor);
            blocks.push(Block {
                span: lines[cursor].start..lines[end - 1].end,
                kind,
            });
            cursor = end;
        }
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
        self.blocks.iter().enumerate().filter_map(|(index, block)| {
            let BlockKind::Heading { level, .. } = block.kind else { return None; };
            let end = self.blocks[index + 1..].iter().find(|next| matches!(next.kind, BlockKind::Heading { level: next_level, .. } if next_level <= level)).map_or(self.source.len(), |next| next.span.start);
            Some(Section { heading: index, span: block.span.start..end, body: block.span.end..end })
        }).collect()
    }
    /// Propose replacing a section body, preserving its heading and surrounding bytes.
    ///
    /// # Errors
    /// Rejects a block index that is not a heading in this document.
    pub fn replace_section(&self, heading: usize, body: &str) -> Result<EditPlan, StopReason> {
        let section = self
            .sections()
            .into_iter()
            .find(|section| section.heading == heading)
            .ok_or_else(|| StopReason::InvalidInput("section heading does not exist".into()))?;
        Ok(EditPlan {
            source: self.digest,
            edits: vec![TextEdit {
                range: section.body,
                replacement: body.into(),
            }],
        })
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
fn classify(source: &str, lines: &[Range<usize>], cursor: usize) -> (BlockKind, usize) {
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
        let content_end = lines.get(end).map_or(source.len(), |range| range.start);
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
    if text.starts_with("    ") || text.starts_with('\t') {
        let mut end = cursor + 1;
        while end < lines.len() && {
            let next = line(source, &lines[end]);
            next.starts_with("    ") || next.starts_with('\t') || next.trim().is_empty()
        } {
            end += 1;
        }
        return (BlockKind::IndentedCode, end);
    }
    if let Some(close) = html_closer(text) {
        let mut end = cursor + 1;
        if !text.contains(close) {
            while end < lines.len() {
                let done = line(source, &lines[end]).contains(close);
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
    if text.trim_start().starts_with('[') && text.contains("]:") {
        return (BlockKind::ReferenceDefinition, cursor + 1);
    }
    paragraph(source, lines, cursor)
}
fn paragraph(source: &str, lines: &[Range<usize>], cursor: usize) -> (BlockKind, usize) {
    let text = line(source, &lines[cursor]);
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
        if let Some(alignments) = alignments(next)
            && header.len() == alignments.len()
            && text.contains('|')
        {
            let mut end = cursor + 2;
            let mut rows = Vec::new();
            while end < lines.len()
                && line(source, &lines[end]).contains('|')
                && !line(source, &lines[end]).trim().is_empty()
            {
                rows.push(cells(line(source, &lines[end])));
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
    (BlockKind::Paragraph, cursor + 1)
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
    let separated = rest
        .get(3..)
        .is_some_and(|tail| tail.is_empty() || tail.starts_with([' ', '\t']));
    let checked = if separated && rest.starts_with("[ ]") {
        Some(false)
    } else if separated && (rest.starts_with("[x]") || rest.starts_with("[X]")) {
        Some(true)
    } else {
        None
    };
    Some(BlockKind::ListItem { ordinal, checked })
}
fn html_closer(text: &str) -> Option<&'static str> {
    let text = unindent(text)?;
    if text.starts_with("<!--") {
        Some("-->")
    } else if text.starts_with("<?") {
        Some("?>")
    } else if text.starts_with("<![CDATA[") {
        Some("]]>")
    } else if text.starts_with("<script") {
        Some("</script>")
    } else if text.starts_with("<pre") {
        Some("</pre>")
    } else if text.starts_with("<style") {
        Some("</style>")
    } else {
        None
    }
}
fn cells(text: &str) -> Vec<String> {
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
            result.push(text[start..position].trim().into());
            start = position + 1;
        }
    }
    result.push(text[start..].trim().into());
    if start == text.len() && start > 0 {
        result.pop();
    }
    if text.starts_with('|') {
        result.remove(0);
    }
    result
}
fn alignments(text: &str) -> Option<Vec<Alignment>> {
    cells(text)
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
