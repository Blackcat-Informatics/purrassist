// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Document rendering over source-addressed container blocks.
use super::{Alignment, Block, BlockKind, Dialect, Document, InlineFragment, References, inline};
use crate::{Governor, ScratchString, StopReason, contract::Resource};
use std::fmt::Write;

impl Document {
    /// Render this document with definitions collected from all indexed containers.
    ///
    /// Source bytes remain unchanged. GFM enables its five extensions, including
    /// tag filtering. Other raw HTML and URI schemes are preserved;
    /// this is not a sanitizer. The pinned `CommonMark` 0.31.2 and GFM
    /// 0.29.0.gfm.13 corpora execute all 1,324 examples with their explicit
    /// extension profiles. Corpus agreement is bounded evidence; independent
    /// differential campaigns have separately documented reference-parser limits.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::{Dialect, Document}};
    /// let mut g = Governor::new(Budget::default());
    /// let doc = Document::parse("[Tea]\n\n> [tea]: /tea\n", &mut g)?;
    /// assert_eq!(doc.render_html(Dialect::CommonMark, &mut g)?, "<p><a href=\"/tea\">Tea</a></p>\n<blockquote>\n</blockquote>\n");
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Returns explicit resource exhaustion or cancellation without partial HTML.
    pub fn render_html(
        &self,
        dialect: Dialect,
        governor: &mut Governor,
    ) -> Result<String, StopReason> {
        let extensions = if dialect == Dialect::Gfm {
            super::Extensions::gfm()
        } else {
            super::Extensions::default()
        };
        self.render_with_extensions(dialect, extensions, governor)
    }
    /// Render with an explicit extension profile, including isolated upstream test profiles.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::{Dialect, Document, Extensions}};
    /// let mut g = Governor::new(Budget::default());
    /// let doc = Document::parse("~~tea~~", &mut g)?;
    /// assert_eq!(doc.render_with_extensions(Dialect::Gfm, Extensions::default(), &mut g)?, "<p>~~tea~~</p>\n");
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Returns explicit resource exhaustion or cancellation without partial HTML.
    pub fn render_with_extensions(
        &self,
        dialect: Dialect,
        extensions: super::Extensions,
        governor: &mut Governor,
    ) -> Result<String, StopReason> {
        governor.charge(
            Resource::Work,
            super::size(self.source.len())
                .saturating_mul(16)
                .saturating_add(1),
        )?;
        // The rendered HTML output is charged and reserved through
        // `ScratchString` before any of it is written, rather than growing an
        // ordinary `String` behind an unrelated flat guess; `into_string`
        // permanently retains the charge, since the caller keeps this output.
        let mut buffer = ScratchString::new(governor);
        buffer.reserve(
            usize::try_from(
                super::size(self.source.len())
                    .saturating_mul(64)
                    .saturating_add(256),
            )
            .unwrap_or(usize::MAX),
        )?;
        // The structural index always exposes GFM tables for editing. When
        // Tables is disabled, reinterpret those lines under ordinary block
        // rules so paragraphs, HTML, references, and setext headings compose
        // correctly across a formerly indexed table boundary.
        let projected;
        let blocks = if !extensions.contains(crate::markdown::Extension::Tables)
            && contains_tables(&self.blocks)
        {
            governor.charge(
                Resource::ScratchBytes,
                super::size(self.source.len())
                    .saturating_mul(128)
                    .saturating_add(128),
            )?;
            projected = super::index_blocks(&self.source, governor, false)?;
            projected.as_slice()
        } else {
            self.blocks.as_slice()
        };
        let mut definitions = Vec::new();
        let mut lazy_lines = Vec::new();
        collect(
            &self.source,
            blocks,
            None,
            &mut definitions,
            &mut lazy_lines,
            governor,
        )?;
        let sources: Vec<_> = definitions.iter().map(String::as_str).collect();
        let refs = References::parse_blocks_with_lazy(&sources, &lazy_lines, governor)?;
        let mut renderer = Renderer {
            source: &self.source,
            dialect,
            extensions,
            refs: &refs,
            governor,
            output: buffer.into_string(),
        };
        renderer.blocks(blocks, false, 0, None)?;
        Ok(
            if extensions.contains(crate::markdown::Extension::TagFilter) {
                super::extensions::filter(&renderer.output)
            } else {
                renderer.output
            },
        )
    }
}
fn contains_tables(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| {
        matches!(block.kind, BlockKind::Table { .. }) || contains_tables(&block.children)
    })
}

fn text(source: &str, block: &Block) -> String {
    let mut result = String::new();
    for (range, padding) in block.lines.iter().zip(&block.line_padding) {
        result.extend(std::iter::repeat_n(' ', *padding));
        result.push_str(&source[range.clone()]);
    }
    result
}

// Block indentation is consumed before inline parsing, except for lazy
// continuation lines whose container prefix was omitted. Those retain their
// literal indentation inside raw HTML and code spans.
// Compare residual tab padding too: an explicitly consumed prefix can restore
// to the same original byte offset as its parent after a partial tab.
pub(super) fn paragraph_text(source: &str, block: &Block, parent: Option<&Block>) -> String {
    let mut result = String::new();
    for (range, padding) in block.lines.iter().zip(&block.line_padding) {
        let text = &source[range.clone()];
        let lazy = parent.is_some_and(|parent| {
            parent
                .lines
                .binary_search_by_key(&range.start, |outer| outer.start)
                .is_ok_and(|index| parent.line_padding[index] == *padding)
        });
        if lazy {
            result.extend(std::iter::repeat_n(' ', *padding));
            result.push_str(text);
        } else {
            result.push_str(text.trim_start_matches([' ', '\t']));
        }
    }
    result
}

fn collect(
    source: &str,
    blocks: &[Block],
    parent: Option<&Block>,
    definitions: &mut Vec<String>,
    lazy_lines: &mut Vec<Vec<usize>>,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    for block in blocks {
        if matches!(block.kind, BlockKind::ReferenceDefinition) {
            lazy_lines.push(super::references::lazy_line_starts(
                block, parent, governor,
            )?);
            definitions.push(text(source, block));
        }
        collect(
            source,
            &block.children,
            Some(block),
            definitions,
            lazy_lines,
            governor,
        )?;
    }
    Ok(())
}
struct Renderer<'a> {
    source: &'a str,
    dialect: Dialect,
    extensions: super::Extensions,
    refs: &'a References,
    governor: &'a mut Governor,
    output: String,
}
impl Renderer<'_> {
    fn inline(&mut self, source: &str) -> Result<(), StopReason> {
        let fragment = InlineFragment::parse_with_extensions(
            source,
            self.dialect,
            self.refs,
            self.extensions,
            self.governor,
        )?;
        self.output.push_str(&fragment.render_html(self.governor)?);
        Ok(())
    }
    fn blocks(
        &mut self,
        blocks: &[Block],
        tight: bool,
        depth: u32,
        parent: Option<&Block>,
    ) -> Result<(), StopReason> {
        self.governor.charge(Resource::Depth, u64::from(depth))?;
        let mut position = 0;
        while position < blocks.len() {
            self.governor.charge(Resource::Work, 1)?;
            let block = &blocks[position];
            if matches!(block.kind, BlockKind::ListItem { .. }) {
                position = self.list(blocks, position, depth)?;
            } else {
                self.block(block, tight, depth, parent)?;
                position += 1;
            }
        }
        Ok(())
    }
    fn block(
        &mut self,
        block: &Block,
        tight: bool,
        depth: u32,
        parent: Option<&Block>,
    ) -> Result<(), StopReason> {
        match &block.kind {
            BlockKind::Blank | BlockKind::ReferenceDefinition => {}
            BlockKind::Heading { level, title } => {
                write!(self.output, "<h{level}>").expect("String write");
                self.inline(title)?;
                writeln!(self.output, "</h{level}>").expect("String write");
            }
            BlockKind::ThematicBreak => self.output.push_str("<hr />\n"),
            BlockKind::Html => {
                self.output
                    .push_str(&inline::normalize_lines(&text(self.source, block)));
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
            }
            BlockKind::Quote => {
                self.output.push_str("<blockquote>\n");
                self.blocks(&block.children, false, depth + 1, Some(block))?;
                self.output.push_str("</blockquote>\n");
            }
            BlockKind::CodeFence { info, closed, .. } => self.code(block, Some((info, *closed))),
            BlockKind::IndentedCode => self.code(block, None),
            BlockKind::Table {
                header,
                alignments,
                rows,
            } if self.extensions.contains(crate::markdown::Extension::Tables) => {
                self.table(header, alignments, rows)?;
            }
            BlockKind::Paragraph | BlockKind::Table { .. } => {
                self.paragraph(&paragraph_text(self.source, block, parent), tight)?;
            }
            BlockKind::ListItem { .. } => unreachable!("grouped by blocks"),
        }
        Ok(())
    }
    fn paragraph(&mut self, source: &str, tight: bool) -> Result<(), StopReason> {
        if !tight {
            self.output.push_str("<p>");
        }
        self.inline(source)?;
        if !tight {
            self.output.push_str("</p>");
        }
        self.output.push('\n');
        Ok(())
    }
    fn code(&mut self, block: &Block, fence: Option<(&String, bool)>) {
        self.output.push_str("<pre><code");
        let (start, end, indent) = if let Some((info, closed)) = fence {
            let info = inline::entities::text(info);
            if let Some(language) = info.split_whitespace().next() {
                self.output.push_str(" class=\"language-");
                inline::escape(language, &mut self.output);
                self.output.push('"');
            }
            let first = &self.source[block.lines[0].clone()];
            (
                1,
                block.lines.len() - usize::from(closed),
                first.bytes().take_while(|byte| *byte == b' ').count(),
            )
        } else {
            (0, block.lines.len(), 4)
        };
        self.output.push('>');
        let mut body = String::new();
        for (range, padding) in block.lines[start..end]
            .iter()
            .zip(&block.line_padding[start..end])
        {
            let original = inline::normalize_lines(&self.source[range.clone()]);
            body.push_str(&" ".repeat(padding.saturating_sub(indent)));
            body.push_str(&unindent(
                &original,
                indent.saturating_sub(*padding),
                column(self.source, range.start),
            ));
        }
        if fence.is_none() {
            while body.ends_with("\n\n") {
                body.pop();
            }
        }
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        inline::escape(&body, &mut self.output);
        self.output.push_str("</code></pre>\n");
    }
    fn list(&mut self, blocks: &[Block], start: usize, depth: u32) -> Result<usize, StopReason> {
        let marker = self.marker(&blocks[start]);
        let mut end = start + 1;
        let mut loose = false;
        while end < blocks.len() {
            let mut next = end;
            while next < blocks.len() && matches!(blocks[next].kind, BlockKind::Blank) {
                next += 1;
            }
            if next < blocks.len()
                && matches!(blocks[next].kind, BlockKind::ListItem { .. })
                && self.marker(&blocks[next]) == marker
            {
                loose |= next != end;
                end = next + 1;
            } else {
                break;
            }
        }
        loose |= blocks[start..end].iter().any(|block| {
            block
                .children
                .iter()
                .any(|child| matches!(child.kind, BlockKind::Blank))
        });
        let BlockKind::ListItem { ordinal, .. } = blocks[start].kind else {
            unreachable!()
        };
        let tag = if ordinal.is_some() { "ol" } else { "ul" };
        write!(self.output, "<{tag}").expect("String write");
        if let Some(ordinal) = ordinal
            && ordinal != 1
        {
            write!(self.output, " start=\"{ordinal}\"").expect("String write");
        }
        self.output.push_str(">\n");
        for block in &blocks[start..end] {
            if !matches!(block.kind, BlockKind::ListItem { .. }) {
                continue;
            }
            self.output.push_str("<li>");
            let visible = block.children.iter().any(|child| {
                !matches!(
                    child.kind,
                    BlockKind::ReferenceDefinition | BlockKind::Blank
                )
            });
            let first_visible = block.children.iter().position(|child| {
                !matches!(
                    child.kind,
                    BlockKind::ReferenceDefinition | BlockKind::Blank
                )
            });
            let paragraph = first_visible.is_some_and(|position| {
                matches!(block.children[position].kind, BlockKind::Paragraph)
            });
            if visible && (loose || !paragraph) {
                self.output.push('\n');
            }
            if self
                .extensions
                .contains(crate::markdown::Extension::TaskLists)
                && paragraph
                && let BlockKind::ListItem {
                    checked: Some(checked),
                    ..
                } = block.kind
            {
                self.task(
                    block,
                    first_visible.expect("task has a first paragraph"),
                    loose,
                    depth,
                    checked,
                )?;
            } else {
                self.blocks(&block.children, !loose, depth + 1, Some(block))?;
            }
            if !loose
                && block
                    .children
                    .last()
                    .is_some_and(|child| matches!(child.kind, BlockKind::Paragraph))
            {
                self.output.pop();
            }
            self.output.push_str("</li>\n");
        }
        writeln!(self.output, "</{tag}>").expect("String write");
        Ok(end)
    }
    fn task(
        &mut self,
        block: &Block,
        position: usize,
        loose: bool,
        depth: u32,
        checked: bool,
    ) -> Result<(), StopReason> {
        if loose {
            self.output.push_str("<p>");
        }
        self.output.push_str(if checked {
            "<input checked=\"\" disabled=\"\" type=\"checkbox\"> "
        } else {
            "<input disabled=\"\" type=\"checkbox\"> "
        });
        let source = paragraph_text(self.source, &block.children[position], Some(block));
        let source = source.trim_start_matches([' ', '\t']);
        let (_, width) = super::task_marker(source).expect("indexed task marker");
        self.inline(source[width..].trim_start_matches([' ', '\t', '\r', '\n']))?;
        if loose {
            self.output.push_str("</p>");
        }
        self.output.push('\n');
        self.blocks(
            &block.children[position + 1..],
            !loose,
            depth + 1,
            Some(block),
        )?;
        Ok(())
    }
    fn marker(&self, block: &Block) -> u8 {
        self.source[block.lines[0].clone()]
            .trim_start()
            .bytes()
            .find(|byte| !byte.is_ascii_digit())
            .expect("list marker")
    }
    fn table(
        &mut self,
        header: &[String],
        alignments: &[Alignment],
        rows: &[Vec<String>],
    ) -> Result<(), StopReason> {
        self.output.push_str("<table>\n<thead>\n<tr>\n");
        self.cells(header, alignments, "th")?;
        self.output.push_str("</tr>\n</thead>\n");
        if !rows.is_empty() {
            self.output.push_str("<tbody>\n");
            for row in rows {
                self.output.push_str("<tr>\n");
                self.cells(row, alignments, "td")?;
                self.output.push_str("</tr>\n");
            }
            self.output.push_str("</tbody>\n");
        }
        self.output.push_str("</table>\n");
        Ok(())
    }
    fn cells(
        &mut self,
        cells: &[String],
        alignments: &[Alignment],
        tag: &str,
    ) -> Result<(), StopReason> {
        for (index, alignment) in alignments.iter().enumerate() {
            write!(self.output, "<{tag}").expect("String write");
            let value = match alignment {
                Alignment::Default => None,
                Alignment::Left => Some("left"),
                Alignment::Right => Some("right"),
                Alignment::Center => Some("center"),
            };
            if let Some(value) = value {
                write!(self.output, " align=\"{value}\"").expect("String write");
            }
            self.output.push('>');
            self.inline(
                &cells
                    .get(index)
                    .map_or("", String::as_str)
                    .replace("\\|", "|"),
            )?;
            writeln!(self.output, "</{tag}>").expect("String write");
        }
        Ok(())
    }
}
fn unindent(source: &str, required: usize, initial: usize) -> String {
    let required = required + initial;
    let mut column = initial;
    let mut end = 0;
    for byte in source.bytes() {
        if column >= required {
            break;
        }
        match byte {
            b' ' => column += 1,
            b'\t' => column += 4 - column % 4,
            _ => break,
        }
        end += 1;
    }
    let mut result = " ".repeat(column.saturating_sub(required));
    result.push_str(&source[end..]);
    result
}

fn column(source: &str, position: usize) -> usize {
    let prefix = &source[..position];
    let start = prefix.rfind(['\r', '\n']).map_or(0, |offset| offset + 1);
    prefix[start..].chars().fold(0, |column, character| {
        if character == '\t' {
            column + 4 - column % 4
        } else {
            column + 1
        }
    })
}
