// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Tab-column parsing view mapped back to exact original byte ranges.
use super::{Block, BlockKind};
use std::ops::Range;

pub(super) struct Projection {
    pub text: String,
    offsets: Vec<usize>,
    padding: Vec<usize>,
}
impl Projection {
    pub(super) fn new(source: &str) -> Self {
        let mut result = Self {
            text: String::new(),
            offsets: Vec::new(),
            padding: Vec::new(),
        };
        let mut column = 0;
        for (offset, character) in source.char_indices() {
            if character == '\t' {
                let width = 4 - column % 4;
                result.text.push_str(&" ".repeat(width));
                for index in 0..width {
                    result.offsets.push(offset + usize::from(index != 0));
                    result
                        .padding
                        .push(if index == 0 { 0 } else { width - index });
                }
                column += width;
            } else {
                result.text.push(character);
                for byte in 0..character.len_utf8() {
                    result.offsets.push(offset + byte);
                    result.padding.push(0);
                }
                if matches!(character, '\r' | '\n') {
                    column = 0;
                } else {
                    column += 1;
                }
            }
        }
        result.offsets.push(source.len());
        result.padding.push(0);
        result
    }
    fn range(&self, range: &Range<usize>) -> Range<usize> {
        self.offsets[range.start]..self.offsets[range.end]
    }
    pub(super) fn restore(&self, source: &str, blocks: &mut [Block]) {
        self.restore_with_parent(source, blocks, None);
    }
    fn restore_with_parent(
        &self,
        source: &str,
        blocks: &mut [Block],
        parent: Option<(&[Range<usize>], &[usize])>,
    ) {
        let mut parent_position = 0;
        for block in blocks {
            block.span = self.range(&block.span);
            for (range, padding) in block.lines.iter_mut().zip(&mut block.line_padding) {
                *padding = self.padding[range.start];
                *range = self.range(range);
            }
            match &mut block.kind {
                BlockKind::Heading { title, .. } => {
                    let first = leading(source, &block.lines[0], block.line_padding[0]);
                    *title = if let Some((_, title)) = super::heading(&first) {
                        title
                    } else {
                        let mut lazy = Vec::new();
                        if let Some((outer, padding)) = parent {
                            for (range, inner_padding) in
                                block.lines.iter().zip(&block.line_padding)
                            {
                                while outer
                                    .get(parent_position)
                                    .is_some_and(|line| line.start < range.start)
                                {
                                    parent_position += 1;
                                }
                                if outer
                                    .get(parent_position)
                                    .is_some_and(|line| line.start == range.start)
                                    && padding[parent_position] == *inner_padding
                                {
                                    lazy.push(range.start);
                                }
                            }
                        }
                        super::setext_title(
                            source,
                            &block.lines[..block.lines.len() - 1],
                            &lazy,
                            &block.line_padding,
                        )
                    };
                }
                BlockKind::CodeFence { info, content, .. } => {
                    *content = self.range(content);
                    let first = leading(source, &block.lines[0], block.line_padding[0]);
                    if let Some((_, _, original)) = super::fence(&first) {
                        *info = original.into();
                    }
                }
                BlockKind::Table { header, rows, .. } => {
                    *header = super::cells(super::line(source, &block.lines[0]));
                    *rows = block.lines[2..]
                        .iter()
                        .map(|range| super::cells(super::line(source, range)))
                        .collect();
                }
                _ => {}
            }
            self.restore_with_parent(
                source,
                &mut block.children,
                Some((&block.lines, &block.line_padding)),
            );
        }
    }
}

fn leading(source: &str, range: &Range<usize>, padding: usize) -> String {
    let raw = super::line(source, range);
    let prefix = &source[..range.start];
    let start = prefix.rfind(['\r', '\n']).map_or(0, |offset| offset + 1);
    let mut column = prefix[start..].chars().fold(0, |column, character| {
        if character == '\t' {
            column + 4 - column % 4
        } else {
            column + 1
        }
    });
    let mut result = " ".repeat(padding);
    let mut consumed = 0;
    for byte in raw.bytes() {
        match byte {
            b' ' => {
                result.push(' ');
                column += 1;
            }
            b'\t' => {
                let width = 4 - column % 4;
                result.push_str(&" ".repeat(width));
                column += width;
            }
            _ => break,
        }
        consumed += 1;
    }
    result.push_str(&raw[consumed..]);
    result
}
