// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Nested container indexing with original-source ranges and bounded recursion.
use super::{
    Block, BlockKind, code_indent, fence, heading, html, interrupts_paragraph, line, list_item,
    setext, size, thematic, unindent,
};
use crate::{Governor, StopReason, contract::Resource};
use std::ops::Range;

pub(super) fn index(
    source: &str,
    lines: &[Range<usize>],
    governor: &mut Governor,
    depth: u32,
) -> Result<Vec<Block>, StopReason> {
    governor.charge(Resource::Depth, u64::from(depth))?;
    governor.charge(
        Resource::Work,
        lines.iter().map(|range| size(range.len())).sum(),
    )?;
    governor.charge(
        Resource::ScratchBytes,
        size(lines.len()).saturating_mul(256),
    )?;
    let mut blocks = Vec::new();
    let mut cursor = 0;
    while cursor < lines.len() {
        let (mut kind, mut end) = definition_end(source, lines, cursor, governor)?.map_or_else(
            || super::classify(source, lines, cursor),
            |end| (BlockKind::ReferenceDefinition, end),
        );
        let children = match &kind {
            BlockKind::Quote => {
                let (inner, stop) = quote_lines(source, lines, cursor);
                end = stop;
                let mut children = index(source, &inner, governor, depth + 1)?;
                let lazy: Vec<_> = inner
                    .iter()
                    .filter(|range| lines.iter().any(|outer| outer.start == range.start))
                    .map(|range| range.start)
                    .collect();
                demote_lazy(source, &mut children, &lazy);
                children
            }
            BlockKind::ListItem { .. } => {
                let (inner, stop) = item_lines(source, lines, cursor);
                end = stop;
                let mut children = index(source, &inner, governor, depth + 1)?;
                if children
                    .first()
                    .is_some_and(|block| matches!(block.kind, BlockKind::Blank))
                {
                    children.remove(0);
                }
                children
            }
            _ => Vec::new(),
        };
        if let BlockKind::ListItem { checked, .. } = &mut kind
            && !children
                .iter()
                .find(|child| !matches!(child.kind, BlockKind::Blank))
                .is_some_and(|child| matches!(child.kind, BlockKind::Paragraph))
        {
            *checked = None;
        }
        blocks.push(Block {
            span: lines[cursor].start..lines[end - 1].end,
            lines: lines[cursor..end].to_vec(),
            line_padding: vec![0; end - cursor],
            kind,
            children,
        });
        cursor = end;
    }
    Ok(blocks)
}
fn quote_prefix(text: &str) -> Option<usize> {
    let stripped = unindent(text)?;
    let rest = stripped.strip_prefix('>')?;
    Some(text.len() - rest.len() + usize::from(rest.starts_with([' ', '\t'])))
}
fn quote_lines(source: &str, lines: &[Range<usize>], cursor: usize) -> (Vec<Range<usize>>, usize) {
    let mut inner = Vec::new();
    let mut end = cursor;
    let mut continuation = Continuation::default();
    while end < lines.len() {
        let text = line(source, &lines[end]);
        if let Some(prefix) = quote_prefix(text) {
            let range = lines[end].start + prefix..lines[end].end;
            continuation.feed(line(source, &range));
            inner.push(range);
        } else if continuation.paragraph && !interrupts_paragraph(text) {
            inner.push(lines[end].clone());
        } else {
            break;
        }
        end += 1;
    }
    (inner, end)
}
fn item_lines(source: &str, lines: &[Range<usize>], cursor: usize) -> (Vec<Range<usize>>, usize) {
    let text = line(source, &lines[cursor]);
    let (prefix, indent) = item_prefix(text);
    let first = lines[cursor].start + prefix..lines[cursor].end;
    let mut continuation = Continuation::default();
    continuation.feed(line(source, &first));
    let mut inner = vec![first.clone()];
    let mut end = cursor + 1;
    while end < lines.len() {
        let text = line(source, &lines[end]);
        if text.trim_matches([' ', '\t']).is_empty() {
            if inner.len() == 1 && line(source, &first).trim_matches([' ', '\t']).is_empty() {
                break;
            }
            inner.push(lines[end].clone());
            continuation.feed("");
        } else if let Some(prefix) = strip_indent(text, indent) {
            let range = lines[end].start + prefix..lines[end].end;
            continuation.feed(line(source, &range));
            inner.push(range);
        } else if continuation.paragraph && list_item(text).is_none() && !interrupts_paragraph(text)
        {
            inner.push(lines[end].clone());
        } else {
            break;
        }
        end += 1;
    }
    // Trailing blanks separate outer blocks and must not migrate into the item.
    while inner
        .last()
        .is_some_and(|range| line(source, range).trim_matches([' ', '\t']).is_empty())
        && inner.len() > 1
    {
        inner.pop();
        end -= 1;
    }
    (inner, end)
}
fn item_prefix(text: &str) -> (usize, usize) {
    let stripped = unindent(text).expect("recognized list item");
    let leading = text.len() - stripped.len();
    let width = if stripped.as_bytes()[0].is_ascii_digit() {
        stripped.bytes().take_while(u8::is_ascii_digit).count() + 1
    } else {
        1
    };
    let marker_end = leading + width;
    let mut end = marker_end;
    let mut column = marker_end;
    while let Some(byte) = text.as_bytes().get(end) {
        match byte {
            b' ' => column += 1,
            b'\t' => column += 4 - column % 4,
            _ => break,
        }
        end += 1;
    }
    let padding = column - marker_end;
    if !(1..=4).contains(&padding) || end == text.len() {
        (marker_end + usize::from(end > marker_end), marker_end + 1)
    } else {
        (end, column)
    }
}
fn strip_indent(text: &str, required: usize) -> Option<usize> {
    let mut column = 0;
    for (position, byte) in text.bytes().enumerate() {
        match byte {
            b' ' => column += 1,
            b'\t' => column += 4 - column % 4,
            _ => return None,
        }
        if column >= required {
            return Some(position + 1);
        }
    }
    None
}
#[derive(Default)]
struct Continuation {
    paragraph: bool,
    fence: Option<(char, usize)>,
    html: Option<html::End>,
    prefixes: Vec<Prefix>,
}
#[derive(Clone, Copy)]
enum Prefix {
    Quote,
    Item(usize),
}
impl Continuation {
    fn feed(&mut self, mut text: &str) {
        for index in 0..self.prefixes.len() {
            let prefix = match self.prefixes[index] {
                Prefix::Quote => quote_prefix(text),
                Prefix::Item(indent) => {
                    if text.trim_matches([' ', '\t']).is_empty() {
                        self.paragraph = false;
                        return;
                    }
                    strip_indent(text, indent)
                }
            };
            if let Some(prefix) = prefix {
                text = &text[prefix..];
            } else {
                if self.paragraph && list_item(text).is_none() && !interrupts_paragraph(text) {
                    return;
                }
                self.prefixes.truncate(index);
                self.fence = None;
                self.html = None;
                self.paragraph = false;
                break;
            }
        }
        if let Some((marker, count)) = self.fence {
            if fence(text).is_some_and(|(other, width, rest)| {
                other == marker && width >= count && rest.is_empty()
            }) {
                self.fence = None;
            }
            return;
        }
        if let Some(ending) = self.html {
            if ending.closes(text) {
                self.html = None;
            }
            return;
        }
        // Missing outer markers may continue the deepest open paragraph.
        loop {
            if let Some(prefix) = quote_prefix(text) {
                self.prefixes.push(Prefix::Quote);
                text = &text[prefix..];
                self.paragraph = false;
            } else if list_item(text).is_some()
                && !thematic(text)
                && (!self.paragraph || interrupts_paragraph(text))
            {
                let (prefix, indent) = item_prefix(text);
                self.prefixes.push(Prefix::Item(indent));
                text = &text[prefix..];
                self.paragraph = false;
            } else {
                break;
            }
        }
        if let Some((marker, count, _)) = fence(text) {
            self.fence = Some((marker, count));
            self.paragraph = false;
        } else if let Some(ending) = html::start(text, self.paragraph) {
            self.html = (!ending.closes(text)).then_some(ending);
            self.paragraph = false;
        } else if text.trim_matches([' ', '\t']).is_empty()
            || heading(text).is_some()
            || thematic(text)
            || (self.paragraph && setext(text).is_some())
        {
            self.paragraph = false;
        } else {
            self.paragraph = self.paragraph || !code_indent(text);
        }
    }
}

fn definition_end(
    source: &str,
    lines: &[Range<usize>],
    cursor: usize,
    governor: &mut Governor,
) -> Result<Option<usize>, StopReason> {
    if !unindent(line(source, &lines[cursor])).is_some_and(|text| text.starts_with('[')) {
        return Ok(None);
    }
    let end = lines[cursor..]
        .iter()
        .position(|range| line(source, range).trim_matches([' ', '\t']).is_empty())
        .map_or(lines.len(), |offset| cursor + offset);
    let size: usize = lines[cursor..end].iter().map(Range::len).sum();
    governor.charge(Resource::ScratchBytes, super::size(size))?;
    governor.charge(Resource::Work, super::size(size))?;
    let text: String = lines[cursor..end]
        .iter()
        .map(|range| &source[range.clone()])
        .collect();
    let definitions = super::References::parse_blocks(&[&text], governor)?;
    let consumed = definitions.consumed()[0];
    if consumed == 0 {
        return Ok(None);
    }
    let mut bytes = 0;
    for (offset, range) in lines[cursor..end].iter().enumerate() {
        bytes += range.len();
        if bytes == consumed {
            return Ok(Some(cursor + offset + 1));
        }
    }
    Ok(None)
}

fn demote_lazy(source: &str, blocks: &mut [Block], lazy: &[usize]) {
    for block in blocks {
        if matches!(block.kind, BlockKind::Heading { .. })
            && block.lines.len() > 1
            && block
                .lines
                .last()
                .is_some_and(|range| lazy.contains(&range.start))
            && heading(line(source, &block.lines[0])).is_none()
        {
            block.kind = BlockKind::Paragraph;
        }
        demote_lazy(source, &mut block.children, lazy);
    }
}
