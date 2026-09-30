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
    tables: bool,
) -> Result<Vec<Block>, StopReason> {
    index_with_lazy(source, lines, governor, depth, tables, &[])
}
fn index_with_lazy(
    source: &str,
    lines: &[Range<usize>],
    governor: &mut Governor,
    depth: u32,
    tables: bool,
    lazy: &[usize],
) -> Result<Vec<Block>, StopReason> {
    governor.charge(Resource::Depth, u64::from(depth))?;
    governor.charge(
        Resource::Work,
        lines
            .iter()
            .map(|range| size(range.len()))
            .sum::<u64>()
            .saturating_add(size(lines.len()).saturating_mul(4)),
    )?;
    governor.charge(
        Resource::ScratchBytes,
        size(lines.len()).saturating_mul(256),
    )?;
    let mut blocks = Vec::new();
    let mut cursor = 0;
    let mut lazy_position = 0;
    while cursor < lines.len() {
        while lazy
            .get(lazy_position)
            .is_some_and(|start| *start < lines[cursor].start)
        {
            lazy_position += 1;
        }
        let remaining_lazy = &lazy[lazy_position..];
        let (kind, mut end) =
            definition_end(source, lines, cursor, governor, tables, remaining_lazy)?.map_or_else(
                || {
                    let text = line(source, &lines[cursor]);
                    // Definitions are removed from the start of an open paragraph,
                    // rather than ending it. Preserve that context for its remainder:
                    // indentation, type-7 HTML and noninterrupting list markers stay
                    // paragraph text. An otherwise empty setext heading also leaves
                    // its underline as paragraph text after definitions are removed.
                    // A table delimiter can split an existing paragraph before
                    // its final header line. That line keeps its paragraph
                    // admission even when indented too far to begin a new one;
                    // standalone indented input still follows code precedence.
                    if blocks.last().is_some_and(|block: &Block| {
                        matches!(block.kind, BlockKind::ReferenceDefinition)
                            || (tables
                                && code_indent(text)
                                && matches!(block.kind, BlockKind::Paragraph))
                    }) && (!interrupts_paragraph(text) || setext(text).is_some())
                    {
                        super::paragraph(source, lines, cursor, tables, remaining_lazy)
                    } else {
                        super::classify(source, lines, cursor, tables, remaining_lazy)
                    }
                },
                |end| (BlockKind::ReferenceDefinition, end),
            );
        let children = match &kind {
            BlockKind::Quote => {
                let (inner, stop) =
                    quote_lines(source, lines, cursor, tables, &lazy[lazy_position..]);
                end = stop;
                let lazy = child_lazy_lines(
                    &lines[cursor..end],
                    &inner,
                    &lazy[lazy_position..],
                    governor,
                )?;
                index_with_lazy(source, &inner, governor, depth + 1, tables, &lazy)?
            }
            BlockKind::ListItem { .. } => {
                let (inner, stop) =
                    item_lines(source, lines, cursor, tables, &lazy[lazy_position..]);
                end = stop;
                let lazy = child_lazy_lines(
                    &lines[cursor..end],
                    &inner,
                    &lazy[lazy_position..],
                    governor,
                )?;
                let mut children =
                    index_with_lazy(source, &inner, governor, depth + 1, tables, &lazy)?;
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
fn child_lazy_lines(
    outer: &[Range<usize>],
    inner: &[Range<usize>],
    inherited: &[usize],
    governor: &mut Governor,
) -> Result<Vec<usize>, StopReason> {
    // Both line views are ordered. Merge them once: a removed local prefix
    // cannot make a line explicit if an enclosing container admitted it lazily.
    // Storage is covered by index_with_lazy's per-line scratch reservation.
    governor.charge(Resource::Work, size(outer.len()).saturating_mul(3))?;
    let mut lazy = Vec::new();
    let mut parent = 0;
    let mut inherited_position = 0;
    for range in inner {
        while parent + 1 < outer.len() && outer[parent + 1].start <= range.start {
            parent += 1;
        }
        while inherited
            .get(inherited_position)
            .is_some_and(|start| *start < outer[parent].start)
        {
            inherited_position += 1;
        }
        if range.start == outer[parent].start
            || inherited.get(inherited_position) == Some(&outer[parent].start)
        {
            lazy.push(range.start);
        }
    }
    Ok(lazy)
}
fn quote_prefix(text: &str) -> Option<usize> {
    let stripped = unindent(text)?;
    let rest = stripped.strip_prefix('>')?;
    Some(text.len() - rest.len() + usize::from(rest.starts_with([' ', '\t'])))
}
fn quote_lines(
    source: &str,
    lines: &[Range<usize>],
    cursor: usize,
    tables: bool,
    lazy: &[usize],
) -> (Vec<Range<usize>>, usize) {
    let mut inner = Vec::new();
    let mut end = cursor;
    let mut lazy_position = 0;
    let mut continuation = Continuation {
        table: TableContinuation {
            enabled: tables,
            ..TableContinuation::default()
        },
        ..Continuation::default()
    };
    while end < lines.len() {
        let text = line(source, &lines[end]);
        let inherited = lazy.get(lazy_position) == Some(&lines[end].start);
        if inherited && !text.trim_matches([' ', '\t']).is_empty() {
            // An ancestor's missing prefix admits this line as literal paragraph
            // continuation. Its indentation cannot supply a local block prefix.
            continuation.lazy(text);
            inner.push(lines[end].clone());
        } else if let Some(prefix) = quote_prefix(text) {
            let range = lines[end].start + prefix..lines[end].end;
            continuation.feed(line(source, &range));
            inner.push(range);
        } else if continuation.paragraph && list_item(text).is_none() && !interrupts_paragraph(text)
        {
            continuation.lazy(text);
            inner.push(lines[end].clone());
        } else {
            break;
        }
        lazy_position += usize::from(inherited);
        end += 1;
    }
    (inner, end)
}
fn item_lines(
    source: &str,
    lines: &[Range<usize>],
    cursor: usize,
    tables: bool,
    lazy: &[usize],
) -> (Vec<Range<usize>>, usize) {
    let text = line(source, &lines[cursor]);
    let (prefix, indent) = item_prefix(text);
    let first = lines[cursor].start + prefix..lines[cursor].end;
    let mut continuation = Continuation {
        table: TableContinuation {
            enabled: tables,
            ..TableContinuation::default()
        },
        ..Continuation::default()
    };
    continuation.feed(line(source, &first));
    let mut inner = vec![first.clone()];
    let mut end = cursor + 1;
    let mut lazy_position = usize::from(lazy.first() == Some(&lines[cursor].start));
    while end < lines.len() {
        let text = line(source, &lines[end]);
        let inherited = lazy.get(lazy_position) == Some(&lines[end].start);
        if text.trim_matches([' ', '\t']).is_empty() {
            if inner.len() == 1 && line(source, &first).trim_matches([' ', '\t']).is_empty() {
                break;
            }
            inner.push(lines[end].clone());
            continuation.feed("");
        } else if inherited {
            // Preserve the complete ancestor-lazy source line, including spaces
            // meaningful inside code spans and multiline inline HTML.
            continuation.lazy(text);
            inner.push(lines[end].clone());
        } else if let Some(prefix) = strip_indent(text, indent) {
            let range = lines[end].start + prefix..lines[end].end;
            continuation.feed(line(source, &range));
            inner.push(range);
        } else if continuation.paragraph && list_item(text).is_none() && !interrupts_paragraph(text)
        {
            continuation.lazy(text);
            inner.push(lines[end].clone());
        } else {
            break;
        }
        lazy_position += usize::from(inherited);
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
pub(super) fn item_prefix(text: &str) -> (usize, usize) {
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
    table: TableContinuation,
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
    fn lazy(&mut self, text: &str) {
        // Missing markers carry paragraph text and cannot open a table. A
        // later marked delimiter may convert this paragraph's last line into
        // a header, so retain its width without treating a lazy delimiter as
        // an attempted table opening.
        let count = super::cells(text).len();
        self.table.header = (count > 0).then_some(count);
    }
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
                    self.lazy(text);
                    return;
                }
                self.prefixes.truncate(index);
                self.fence = None;
                self.html = None;
                self.paragraph = false;
                self.table.reset();
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
                self.table.reset();
            } else if list_item(text).is_some()
                && !thematic(text)
                && (!self.paragraph || interrupts_paragraph(text))
            {
                let (prefix, indent) = item_prefix(text);
                self.prefixes.push(Prefix::Item(indent));
                text = &text[prefix..];
                self.paragraph = false;
                self.table.reset();
            } else {
                break;
            }
        }
        if self.table.feed(text, self.paragraph) {
            self.paragraph = false;
            return;
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
        self.table.record(text, self.paragraph);
    }
}

#[derive(Default)]
struct TableContinuation {
    enabled: bool,
    open: bool,
    header: Option<usize>,
    rejected: bool,
}
impl TableContinuation {
    const fn reset(&mut self) {
        self.open = false;
        self.header = None;
        self.rejected = false;
    }
    fn feed(&mut self, text: &str, paragraph: bool) -> bool {
        if self.open
            && !interrupts_paragraph(text)
            && !code_indent(text)
            && list_item(text).is_none()
            && html::start(text, false).is_none()
        {
            return true;
        }
        self.open = false;
        if self.enabled
            && paragraph
            && !self.rejected
            && setext(text).is_none()
            && !interrupts_paragraph(text)
            && super::alignments(text).is_some_and(|alignment| self.header == Some(alignment.len()))
        {
            self.open = true;
            self.header = None;
            return true;
        }
        if paragraph && super::alignments(text).is_some() {
            self.rejected = true;
        }
        false
    }
    fn record(&mut self, text: &str, paragraph: bool) {
        if paragraph && !self.rejected {
            let count = super::cells(text).len();
            self.header = (count > 0).then_some(count);
        } else {
            self.header = None;
            if !paragraph {
                self.rejected = false;
            }
        }
    }
}

fn definition_end(
    source: &str,
    lines: &[Range<usize>],
    cursor: usize,
    governor: &mut Governor,
    tables: bool,
    lazy: &[usize],
) -> Result<Option<usize>, StopReason> {
    if !unindent(line(source, &lines[cursor])).is_some_and(|text| text.starts_with('[')) {
        return Ok(None);
    }
    // Reference definitions are prefixes of paragraph content. A destination
    // or title on a later line cannot absorb an interrupting block. Table
    // headers take precedence over definitions; setext underlines are syntax,
    // not candidate destinations or parts of a multiline title.
    let (kind, mut end) = super::paragraph(source, lines, cursor, tables, lazy);
    if matches!(kind, BlockKind::Table { .. }) {
        return Ok(None);
    }
    if matches!(kind, BlockKind::Heading { .. }) {
        end -= 1;
    }
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
