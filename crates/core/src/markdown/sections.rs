// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Reparsed section-body edits with surrounding block preservation.
mod nested;
use super::{Block, BlockKind, Document, line, size};
use crate::{
    Budget, Governor, Outcome, StopReason,
    contract::Resource,
    edits::{EditPlan, TextEdit},
};
pub use nested::SectionSite;
pub(super) use nested::{bounds, prefix, same_shape, same_tail};

/// Where [`Document::insert_section`] places a new root-level section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionInsertion {
    /// Before every existing root block.
    Start,
    /// Immediately after the named root heading's complete section
    /// (including its own subsections).
    After(usize),
}

impl Document {
    /// Replace a root section body using the standard resource budget.
    ///
    /// The heading and all blocks outside its section must retain their structure.
    /// Use [`Self::replace_section_governed`] to share an explicit budget.
    ///
    /// # Errors
    /// Rejects missing headings and replacements that change section boundaries
    /// or surrounding blocks. Returns resource exhaustion without a partial plan.
    pub fn replace_section(&self, heading: usize, body: &str) -> Result<EditPlan, StopReason> {
        self.replace_section_governed(heading, body, &mut Governor::new(Budget::default()))
    }

    /// Replace a root section body verbatim, then reparse the complete proposal.
    ///
    /// Lower-ranked subheadings belong to the body and may change. A new heading
    /// of equal or higher rank, an unterminated construct that captures following
    /// blocks, or changed heading spelling is rejected. The caller supplies line
    /// endings and separators; no newline is silently inserted. An identical body
    /// produces an empty plan. Plans support exact conditional undo.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::Document};
    /// let mut g = Governor::new(Budget::default());
    /// let doc = Document::parse("# Tea\nHot.\n# Coffee\nKeep.\n", &mut g)?;
    /// let plan = doc.replace_section_governed(0, "Cold.\n", &mut g)?;
    /// assert_eq!(plan.edits[0].replacement, "Cold.\n");
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects a missing/non-heading root index or a proposal that changes the
    /// selected heading, its ending boundary, or any outside block's source maps
    /// and classification. Cancellation and resource exhaustion propagate.
    pub fn replace_section_governed(
        &self,
        heading: usize,
        body: &str,
        governor: &mut Governor,
    ) -> Result<EditPlan, StopReason> {
        governor.charge(Resource::InputBytes, size(body.len()))?;
        governor.charge(
            Resource::Work,
            size(self.source.len())
                .saturating_mul(8)
                .saturating_add(size(body.len())),
        )?;
        governor.charge(
            Resource::ScratchBytes,
            size(self.source.len())
                .saturating_mul(128)
                .saturating_add(size(body.len()).saturating_mul(4))
                .saturating_add(512),
        )?;
        let section = self
            .sections()
            .into_iter()
            .find(|section| section.heading == heading)
            .ok_or_else(|| invalid("section heading does not exist"))?;
        if self.source[section.body.clone()] == *body {
            return Ok(EditPlan {
                source: self.digest,
                edits: Vec::new(),
            });
        }
        let mut proposed = self.source.clone();
        proposed.replace_range(section.body.clone(), body);
        let updated = Self::parse(&proposed, governor)?;
        let end = section.body.start + body.len();
        let updated_section = updated
            .sections()
            .into_iter()
            .find(|section| section.heading == heading)
            .ok_or_else(|| invalid("replacement removes the selected heading"))?;
        if updated_section.body != (section.body.start..end)
            || self.blocks[..=heading] != updated.blocks[..=heading]
        {
            return Err(invalid(
                "replacement changes the heading or section boundary",
            ));
        }
        let original_tail = self
            .blocks
            .partition_point(|block| block.span.start < section.body.end);
        let updated_tail = updated
            .blocks
            .partition_point(|block| block.span.start < end);
        let mut expected = self.blocks[original_tail..].to_vec();
        for block in &mut expected {
            shift(block, section.body.end, end);
        }
        if expected != updated.blocks[updated_tail..] {
            return Err(invalid("replacement changes surrounding block structure"));
        }
        Ok(EditPlan {
            source: self.digest,
            edits: vec![TextEdit {
                range: section.body,
                replacement: body.into(),
            }],
        })
    }

    /// Insert a new, well-formed root-level section (a heading through
    /// everything up to its own next same-or-higher-rank heading, or EOF) at
    /// an explicit boundary between existing root blocks.
    ///
    /// `text` must reparse, standalone, into a heading followed by its body;
    /// the caller supplies every separator and line ending, exactly as
    /// [`Self::replace_section_governed`] does for a body alone. The proposal
    /// is reparsed and every block outside the insertion must stay exact.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, Outcome, markdown::{Document, SectionInsertion}};
    /// let mut g = Governor::new(Budget::default());
    /// let doc = Document::parse("# Tea\nHot.\n", &mut g)?;
    /// let plan = doc.insert_section(SectionInsertion::Start, "# Coffee\nBlack.\n", &mut g)?;
    /// let Outcome::Resolved(applied) = plan.apply(doc.source(), Budget::default()).outcome else {
    ///     panic!("apply")
    /// };
    /// assert_eq!(applied.text, "# Coffee\nBlack.\n# Tea\nHot.\n");
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects text that does not begin with a heading, a missing `After`
    /// target, an insertion point that is not already a root block boundary,
    /// and any proposal that changes an existing block. Resource exhaustion
    /// and cancellation propagate without a partial plan.
    pub fn insert_section(
        &self,
        at: SectionInsertion,
        text: &str,
        governor: &mut Governor,
    ) -> Result<EditPlan, StopReason> {
        let position = match at {
            SectionInsertion::Start => 0,
            SectionInsertion::After(heading) => {
                self.sections()
                    .into_iter()
                    .find(|section| section.heading == heading)
                    .ok_or_else(|| invalid("section heading does not exist"))?
                    .span
                    .end
            }
        };
        insert_section_at(self, position, text, governor)
    }

    /// Remove a complete root-level section: its heading, body, and every
    /// nested subsection, returning the byte-exact remainder.
    ///
    /// # Errors
    /// Rejects a missing/non-heading root index and any proposal that
    /// changes a remaining block. Resource exhaustion and cancellation
    /// propagate without a partial plan.
    pub fn remove_section(
        &self,
        heading: usize,
        governor: &mut Governor,
    ) -> Result<EditPlan, StopReason> {
        governor.charge(Resource::Work, size(self.source.len()).saturating_mul(8))?;
        governor.charge(
            Resource::ScratchBytes,
            size(self.source.len())
                .saturating_mul(128)
                .saturating_add(512),
        )?;
        let block = self
            .blocks
            .get(heading)
            .ok_or_else(|| invalid("section heading does not exist"))?;
        if !matches!(block.kind, BlockKind::Heading { .. }) {
            return Err(invalid("selected block is not a heading"));
        }
        let (body, stop) = bounds(&self.source, &self.blocks, heading, self.source.len())?;
        let full_range = block.span.start..body.end;
        let plan = EditPlan {
            source: self.digest,
            edits: vec![TextEdit {
                range: full_range.clone(),
                replacement: String::new(),
            }],
        };
        if full_range.start == full_range.end {
            return Ok(plan);
        }
        let applied = plan.apply_governed(&self.source, governor);
        let Outcome::Resolved(applied) = applied.outcome else {
            return Err(applied.receipt.stopped);
        };
        let updated = Self::parse(&applied.text, governor)?;
        if updated.blocks[..heading] != self.blocks[..heading] {
            return Err(invalid("removal changes preceding blocks"));
        }
        if updated.blocks.len() < heading
            || !same_tail(
                &self.blocks[stop..],
                &updated.blocks[heading..],
                full_range.end,
                full_range.start,
            )
        {
            return Err(invalid("removal changes following blocks"));
        }
        Ok(plan)
    }

    /// Rename a root heading's title, keeping its level and ATX/setext style.
    ///
    /// A setext title spanning several source lines is replaced by the supplied
    /// single-line title. Its first-line indentation, final title-line ending,
    /// exact underline, and every surrounding byte remain intact. The returned
    /// patch identifies the entire replaced title span; its inverse restores
    /// every original title line, including mixed line endings.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, Outcome, markdown::Document};
    /// let source = "Hot\nTea\n---\n\nKeep.\n";
    /// let mut governor = Governor::new(Budget::default());
    /// let document = Document::parse(source, &mut governor)?;
    /// let plan = document.rename_heading(0, "Chai", &mut governor)?;
    /// let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome
    ///     else { panic!("rename must apply") };
    /// assert_eq!(applied.text, "Chai\n---\n\nKeep.\n");
    /// let Outcome::Resolved(restored) = applied.inverse.apply(&applied.text, Budget::default()).outcome
    ///     else { panic!("inverse must apply") };
    /// assert_eq!(restored.text, source);
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects a missing/non-heading root index, a title containing a line
    /// break or not reparsing as the exact requested heading title, and any proposal
    /// that changes the heading's level, style, or another block. Resource
    /// exhaustion and cancellation propagate without a partial plan.
    pub fn rename_heading(
        &self,
        heading: usize,
        title: &str,
        governor: &mut Governor,
    ) -> Result<EditPlan, StopReason> {
        governor.charge(Resource::InputBytes, size(title.len()))?;
        governor.charge(
            Resource::Work,
            size(self.source.len())
                .saturating_mul(4)
                .saturating_add(size(title.len())),
        )?;
        governor.charge(
            Resource::ScratchBytes,
            size(self.source.len())
                .saturating_mul(64)
                .saturating_add(size(title.len()).saturating_mul(4))
                .saturating_add(256),
        )?;
        if title.contains(['\r', '\n']) {
            return Err(invalid(
                "a single-line heading title cannot contain a line break",
            ));
        }
        let block = self
            .blocks
            .get(heading)
            .ok_or_else(|| invalid("section heading does not exist"))?;
        let BlockKind::Heading {
            level,
            title: current,
        } = &block.kind
        else {
            return Err(invalid("selected block is not a heading"));
        };
        if current == title {
            return Ok(EditPlan {
                source: self.digest,
                edits: Vec::new(),
            });
        }
        let setext = block.lines.len() >= 2;
        let level = *level;
        let first_line = &block.lines[0];
        let final_title_line = &block.lines[if setext { block.lines.len() - 2 } else { 0 }];
        let title_range = first_line.start..final_title_line.end;
        let text = line(&self.source, first_line);
        let final_text = line(&self.source, final_title_line);
        let ending = &self.source[final_title_line.clone()][final_text.len()..];
        let indent = text.len() - text.trim_start_matches(' ').len();
        let replacement = if setext {
            format!("{}{title}{ending}", &text[..indent])
        } else {
            format!(
                "{}{} {title}{ending}",
                &text[..indent],
                "#".repeat(level as usize)
            )
        };
        let plan = EditPlan {
            source: self.digest,
            edits: vec![TextEdit {
                range: title_range.clone(),
                replacement: replacement.clone(),
            }],
        };
        let applied = plan.apply_governed(&self.source, governor);
        let Outcome::Resolved(applied) = applied.outcome else {
            return Err(applied.receipt.stopped);
        };
        let updated = Self::parse(&applied.text, governor)?;
        let new_end = title_range.start + replacement.len();
        let expected = BlockKind::Heading {
            level,
            title: title.into(),
        };
        if updated
            .blocks
            .get(heading)
            .is_none_or(|new_block| new_block.kind != expected)
            || updated.blocks[..heading] != self.blocks[..heading]
            || !same_tail(
                &self.blocks[heading + 1..],
                &updated.blocks[heading + 1..],
                title_range.end,
                new_end,
            )
        {
            return Err(invalid(
                "rename changes the heading's level, style, or another block",
            ));
        }
        Ok(plan)
    }

    /// Move a complete root-level section to a new position, optionally
    /// re-leveling only its own top heading; nested subsection headings
    /// keep their existing levels, so the moved section's internal nesting
    /// relative to its new top level is a reported transformation, not a
    /// refusal.
    ///
    /// Setext headings retain their title lines and underline spacing/width at
    /// levels 1 and 2. Levels 3 through 6 convert the heading to ATX, joining
    /// its title lines with spaces and keeping the underline's line ending.
    /// The inverse restores the original spelling, including every title line.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, Outcome, markdown::{Document, SectionInsertion}};
    /// let source = "Hot\nTea\n---\n\nKeep.\n";
    /// let mut g = Governor::new(Budget::default());
    /// let doc = Document::parse(source, &mut g).unwrap();
    /// let plan = doc.move_section(0, SectionInsertion::Start, Some(3), &mut g).unwrap();
    /// let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else { panic!() };
    /// assert_eq!(applied.text, "### Hot Tea\n\nKeep.\n");
    /// ```
    ///
    /// # Errors
    /// Rejects a missing heading, moving a section after itself or into its
    /// own body, a requested level outside 1 through 6, and every error
    /// [`Self::remove_section`] and [`Self::insert_section`] can return.
    pub fn move_section(
        &self,
        heading: usize,
        after: SectionInsertion,
        new_level: Option<u8>,
        governor: &mut Governor,
    ) -> Result<EditPlan, StopReason> {
        // Admit section-index storage and the moved source before either is copied.
        governor.charge(Resource::Work, size(self.source.len()).saturating_add(1))?;
        governor.charge(
            Resource::ScratchBytes,
            size(self.source.len())
                .saturating_mul(128)
                .saturating_add(512),
        )?;
        let section = self
            .sections()
            .into_iter()
            .find(|section| section.heading == heading)
            .ok_or_else(|| invalid("section heading does not exist"))?;
        let target_section = match after {
            SectionInsertion::Start => None,
            SectionInsertion::After(target) => {
                if target == heading {
                    return Err(invalid("cannot move a section after itself"));
                }
                let target_section = self
                    .sections()
                    .into_iter()
                    .find(|section| section.heading == target)
                    .ok_or_else(|| invalid("target section does not exist"))?;
                if section.span.contains(&target_section.span.start) {
                    return Err(invalid("cannot move a section into its own body"));
                }
                Some(target_section)
            }
        };
        let mut moved_text = self.source[section.span.clone()].to_string();
        if let Some(new_level) = new_level {
            let BlockKind::Heading {
                level: old_level, ..
            } = self.blocks[heading].kind
            else {
                unreachable!("section.heading always names a Heading block")
            };
            if new_level != old_level {
                moved_text = relevel_first_heading(&moved_text, new_level, governor)?;
            }
        }
        let removal = self.remove_section(heading, governor)?;
        let removed = removal.apply_governed(&self.source, governor);
        let Outcome::Resolved(removed) = removed.outcome else {
            return Err(removed.receipt.stopped);
        };
        let removed_doc = Self::parse(&removed.text, governor)?;
        let target_position = match target_section {
            None => 0,
            Some(target_section) => {
                // `target_section.span.end` is an offset into the
                // *original* source; removing `section` shifts every
                // position at or after `section.span.end` left by
                // `section.span.len()`. That holds whenever the removed
                // section ends at or before the point we are measuring,
                // regardless of whether `section` sits entirely before
                // `target_section` (disjoint siblings) or is nested inside
                // it (a subsection of the section named as the move
                // target): both leave `target_section.span.end` past the
                // removed span. Only a `section` that starts at or after
                // `target_section.span.end` (entirely after the target's
                // own complete section) leaves that offset unaffected.
                if section.span.end <= target_section.span.end {
                    target_section.span.end - section.span.len()
                } else {
                    target_section.span.end
                }
            }
        };
        let insertion = insert_section_at(&removed_doc, target_position, &moved_text, governor)?;
        removal.compose(&insertion, &self.source, governor)
    }
}

/// Shared insertion logic for [`Document::insert_section`] and
/// [`Document::move_section`] (which inserts into the document that remains
/// after removing the moved section).
fn insert_section_at(
    doc: &Document,
    position: usize,
    text: &str,
    governor: &mut Governor,
) -> Result<EditPlan, StopReason> {
    governor.charge(
        Resource::Work,
        size(doc.source.len())
            .saturating_mul(8)
            .saturating_add(size(text.len())),
    )?;
    governor.charge(
        Resource::ScratchBytes,
        size(doc.source.len())
            .saturating_mul(128)
            .saturating_add(size(text.len()).saturating_mul(4))
            .saturating_add(512),
    )?;
    let intended = Document::parse(text, governor)?;
    let Some(first) = intended.blocks.first() else {
        return Err(invalid("inserted section is empty"));
    };
    if !matches!(first.kind, BlockKind::Heading { .. }) {
        return Err(invalid("inserted section must begin with a heading"));
    }
    if position > doc.source.len() || !doc.source.is_char_boundary(position) {
        return Err(invalid(
            "insertion point is outside the source or splits a UTF-8 scalar",
        ));
    }
    let idx = doc
        .blocks
        .partition_point(|block| block.span.end <= position);
    if idx < doc.blocks.len() && doc.blocks[idx].span.start != position {
        return Err(invalid("insertion point is not a root block boundary"));
    }
    let plan = EditPlan {
        source: doc.digest,
        edits: vec![TextEdit {
            range: position..position,
            replacement: text.into(),
        }],
    };
    let applied = plan.apply_governed(&doc.source, governor);
    let Outcome::Resolved(applied) = applied.outcome else {
        return Err(applied.receipt.stopped);
    };
    let updated = Document::parse(&applied.text, governor)?;
    if updated.blocks[..idx] != doc.blocks[..idx] {
        return Err(invalid("insertion changes preceding blocks"));
    }
    let inserted_count = intended.blocks.len();
    let new_end = position + text.len();
    if updated.blocks.len() < idx + inserted_count
        || !same_shape(
            &intended.source,
            &intended.blocks,
            &updated.source,
            &updated.blocks[idx..idx + inserted_count],
        )
    {
        return Err(invalid(
            "inserted text does not form the intended section shape",
        ));
    }
    if !same_tail(
        &doc.blocks[idx..],
        &updated.blocks[idx + inserted_count..],
        position,
        new_end,
    ) {
        return Err(invalid("insertion changes following blocks"));
    }
    Ok(plan)
}

/// Rewrite a standalone section's top heading, preserving its body exactly.
fn relevel_first_heading(
    text: &str,
    new_level: u8,
    governor: &mut Governor,
) -> Result<String, StopReason> {
    if !(1..=6).contains(&new_level) {
        return Err(invalid("heading level must be between 1 and 6"));
    }
    governor.charge(Resource::Work, size(text.len()).saturating_add(8))?;
    governor.charge(
        Resource::ScratchBytes,
        size(text.len()).saturating_mul(2).saturating_add(16),
    )?;
    let doc = Document::parse(text, governor)?;
    let Some(first) = doc.blocks.first() else {
        return Err(invalid("moved text does not begin with a heading"));
    };
    let BlockKind::Heading { title, .. } = &first.kind else {
        return Err(invalid("moved text does not begin with a heading"));
    };
    let line_range = first.lines[0].clone();
    let line_text = line(text, &line_range);
    let indent = line_text.len() - line_text.trim_start_matches(' ').len();
    let setext = first.lines.len() >= 2;
    let final_range = first.lines.last().expect("a heading has a source line");
    let final_text = line(text, final_range);
    let ending = &text[final_range.clone()][final_text.len()..];
    if setext && new_level <= 2 {
        let marker = if new_level == 1 { '=' } else { '-' };
        let mut result = text[..final_range.start].to_string();
        for character in final_text.chars() {
            result.push(if matches!(character, '=' | '-') {
                marker
            } else {
                character
            });
        }
        result.push_str(ending);
        result.push_str(&text[final_range.end..]);
        return Ok(result);
    }
    let mut result = text[..line_range.start].to_string();
    result.push_str(&line_text[..indent]);
    result.push_str(&"#".repeat(new_level as usize));
    result.push(' ');
    if setext {
        let joined = title.replace('\n', " ");
        // An unescaped trailing hash sequence would become ATX closing syntax.
        // Escape its first hash so its rendered title remains literal.
        let prefix = joined.trim_end_matches('#');
        if prefix.len() != joined.len() && (prefix.is_empty() || prefix.ends_with([' ', '\t'])) {
            result.push_str(prefix);
            result.push('\\');
            result.push_str(&joined[prefix.len()..]);
        } else {
            result.push_str(&joined);
        }
    } else {
        result.push_str(title);
    }
    result.push_str(ending);
    result.push_str(&text[final_range.end..]);
    Ok(result)
}

pub(super) fn shift(block: &mut Block, old: usize, new: usize) {
    let range = |span: &mut std::ops::Range<usize>| {
        span.start = span.start - old + new;
        span.end = span.end - old + new;
    };
    range(&mut block.span);
    for line in &mut block.lines {
        range(line);
    }
    if let BlockKind::CodeFence { content, .. } = &mut block.kind {
        range(content);
    }
    for child in &mut block.children {
        shift(child, old, new);
    }
}
fn invalid(message: &str) -> StopReason {
    StopReason::InvalidInput(message.into())
}
