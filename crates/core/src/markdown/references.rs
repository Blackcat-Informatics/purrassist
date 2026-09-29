// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Document-addressed reference definitions and verified destination/title edits.
use super::{
    Block, BlockKind, Dialect, Document, Inline, InlineFragment, InlineKind, References,
    cell_ranges, line, size,
};
use crate::{
    Governor, Outcome, StopReason,
    contract::Resource,
    digest::Digest,
    edits::{EditPlan, TextEdit},
};
use std::ops::Range;

/// One reference-definition occurrence in the original document, including duplicates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceSite {
    /// Digest binding every range to the complete original document.
    pub source: Digest,
    /// Root/child block indices selecting its reference-definition block.
    pub path: Vec<usize>,
    /// Zero-based occurrence within that block, including shadowed duplicates.
    pub occurrence: usize,
    /// Complete definition range; interior container prefixes are included.
    pub span: Range<usize>,
    /// Unmodified label spelling, with enclosing container prefixes removed.
    pub label: String,
    /// Decoded destination.
    pub destination: String,
    /// Destination syntax range, including any angle delimiters.
    pub destination_span: Range<usize>,
    /// Decoded title, if supplied.
    pub title: Option<String>,
    /// Title syntax range, including delimiters and interior container prefixes.
    pub title_span: Option<Range<usize>>,
}

impl Document {
    /// Index every reference definition in document order using original byte ranges.
    ///
    /// Duplicate labels remain separate occurrences; reference resolution still uses
    /// the first normalized label. Code and HTML blocks do not contribute definitions.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::Document};
    /// let mut g = Governor::new(Budget::default());
    /// let doc = Document::parse("> [tea]: /tea\n", &mut g)?;
    /// let sites = doc.reference_sites(&mut g)?;
    /// assert_eq!(sites[0].path, [0, 0]);
    /// assert_eq!(&doc.source()[sites[0].destination_span.clone()], "/tea");
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Returns explicit work, input, storage, depth exhaustion or cancellation.
    pub fn reference_sites(
        &self,
        governor: &mut Governor,
    ) -> Result<Vec<ReferenceSite>, StopReason> {
        governor.charge(
            Resource::Work,
            size(self.source.len()).saturating_mul(8).saturating_add(1),
        )?;
        governor.charge(
            Resource::ScratchBytes,
            size(self.source.len())
                .saturating_mul(128)
                .saturating_add(256),
        )?;
        let mut result = Vec::new();
        collect(self, &self.blocks, &mut Vec::new(), &mut result, governor)?;
        Ok(result)
    }

    /// Replace one definition's decoded destination and optional title.
    ///
    /// Select by a `reference_sites` block path and occurrence. `None` removes the
    /// title; `Some("")` retains an empty title. Values are encoded as one physical
    /// line using angle/quote delimiters, escapes, and numeric newline/tab entities.
    /// Label spelling and all bytes outside the destination/title region stay exact.
    /// Existing multiline fields may collapse; surrounding container prefixes and
    /// the original terminating newline stay intact. Equal decoded values are a no-op.
    /// All definition labels/values are reparsed and checked in document order.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::Document};
    /// let mut g = Governor::new(Budget::default());
    /// let doc = Document::parse("[tea]: /hot\n", &mut g)?;
    /// let plan = doc.replace_reference(&[0], 0, "/iced", Some("Tea"), &mut g)?;
    /// assert_eq!(plan.edits.len(), 1);
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects missing occurrences, NUL values (which Markdown cannot retain),
    /// and changes to unselected definitions. Exhaustion/cancellation returns no plan.
    pub fn replace_reference(
        &self,
        path: &[usize],
        occurrence: usize,
        destination: &str,
        title: Option<&str>,
        governor: &mut Governor,
    ) -> Result<EditPlan, StopReason> {
        governor.charge(Resource::Work, size(path.len()).saturating_add(1))?;
        governor.charge(Resource::Depth, size(path.len().saturating_sub(1)))?;
        for value in std::iter::once(destination).chain(title) {
            governor.charge(Resource::InputBytes, size(value.len()))?;
            governor.charge(
                Resource::Work,
                size(value.len()).saturating_mul(16).saturating_add(1),
            )?;
            governor.charge(
                Resource::ScratchBytes,
                size(value.len()).saturating_mul(32).saturating_add(128),
            )?;
            if value.contains('\0') {
                return Err(invalid("Markdown reference fields cannot retain NUL"));
            }
        }
        let mut sites = self.reference_sites(governor)?;
        let index = sites
            .iter()
            .position(|site| site.path == path && site.occurrence == occurrence)
            .ok_or_else(|| invalid("reference definition occurrence does not exist"))?;
        let site = &mut sites[index];
        let mut plan = EditPlan {
            source: self.digest,
            edits: Vec::new(),
        };
        if site.destination == destination && site.title.as_deref() == title {
            return Ok(plan);
        }
        let end = site
            .title_span
            .as_ref()
            .map_or(site.destination_span.end, |span| span.end);
        let mut replacement = String::from("<");
        encode(destination, &mut replacement);
        replacement.push('>');
        if let Some(title) = title {
            replacement.push_str(" \"");
            encode(title, &mut replacement);
            replacement.push('"');
        }
        plan.edits.push(TextEdit {
            range: site.destination_span.start..end,
            replacement,
        });
        site.destination = destination.into();
        site.title = title.map(str::to_owned);
        let applied = plan.apply_governed(&self.source, governor);
        let Outcome::Resolved(value) = applied.outcome else {
            return Err(applied.receipt.stopped);
        };
        let updated = Self::parse(&value.text, governor)?.reference_sites(governor)?;
        if sites.len() != updated.len()
            || sites.iter().zip(&updated).any(|(before, after)| {
                before.label != after.label
                    || before.destination != after.destination
                    || before.title != after.title
            })
        {
            return Err(invalid(
                "reference edit changes an unselected definition or field meaning",
            ));
        }
        Ok(plan)
    }

    /// Append a new reference definition at the end of the document.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::Document};
    /// let mut g = Governor::new(Budget::default());
    /// let doc = Document::parse("[tea]\n", &mut g)?;
    /// let plan = doc.add_reference("tea", "/tea", Some("Tea"), &mut g)?;
    /// assert_eq!(plan.edits.len(), 1);
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects an empty label, a label containing brackets, and NUL fields
    /// (which Markdown cannot retain). Resource exhaustion and cancellation
    /// propagate without a partial plan.
    pub fn add_reference(
        &self,
        label: &str,
        destination: &str,
        title: Option<&str>,
        governor: &mut Governor,
    ) -> Result<EditPlan, StopReason> {
        governor.charge(
            Resource::Work,
            size(self.source.len())
                .saturating_mul(8)
                .saturating_add(size(label.len()).saturating_add(size(destination.len()))),
        )?;
        governor.charge(
            Resource::ScratchBytes,
            size(self.source.len())
                .saturating_mul(64)
                .saturating_add(512),
        )?;
        if label.is_empty() || label.contains(['[', ']']) {
            return Err(invalid(
                "reference label must be non-empty and free of brackets",
            ));
        }
        for value in std::iter::once(destination).chain(title) {
            if value.contains('\0') {
                return Err(invalid("Markdown reference fields cannot retain NUL"));
            }
        }
        // Match the document's own prevailing line ending rather than
        // hard-coding one, so the appended definition preserves an existing
        // CRLF (or bare CR) file's spelling.
        let ending = if self.source.contains("\r\n") {
            "\r\n"
        } else if self.source.contains('\r') {
            "\r"
        } else {
            "\n"
        };
        let mut definition = format!("[{label}]: <");
        encode(destination, &mut definition);
        definition.push('>');
        if let Some(title) = title {
            definition.push_str(" \"");
            encode(title, &mut definition);
            definition.push('"');
        }
        definition.push_str(ending);
        // A definition cannot interrupt paragraph text: appending directly
        // after existing content (without an intervening blank line) would
        // be read as a lazy continuation of whatever precedes it, not a new
        // definition. A trailing blank line is required unless the document
        // is empty or already ends with one.
        let already_blank = self.source.ends_with("\n\n")
            || self.source.ends_with("\r\n\r\n")
            || self.source.ends_with("\n\r\n")
            || self.source.ends_with("\r\r");
        let separator = if self.source.is_empty() || already_blank {
            String::new()
        } else if self.source.ends_with(['\n', '\r']) {
            ending.to_string()
        } else {
            format!("{ending}{ending}")
        };
        let position = self.source.len();
        let plan = EditPlan {
            source: self.digest,
            edits: vec![TextEdit {
                range: position..position,
                replacement: format!("{separator}{definition}"),
            }],
        };
        let applied = plan.apply_governed(&self.source, governor);
        let Outcome::Resolved(applied) = applied.outcome else {
            return Err(applied.receipt.stopped);
        };
        let updated = Self::parse(&applied.text, governor)?;
        let before = self.reference_sites(governor)?;
        let after = updated.reference_sites(governor)?;
        if after.len() != before.len() + 1 {
            return Err(invalid(
                "insertion did not add exactly one reference definition",
            ));
        }
        let Some(new_site) = after.last() else {
            return Err(invalid("insertion did not produce a reference definition"));
        };
        if new_site.label != label
            || new_site.destination != destination
            || new_site.title.as_deref() != title
            || before.iter().zip(&after).any(|(b, a)| {
                b.label != a.label || b.destination != a.destination || b.title != a.title
            })
        {
            return Err(invalid(
                "insertion changed an existing reference definition or the new one",
            ));
        }
        Ok(plan)
    }

    /// Relabel a reference definition and rewrite every use that resolves to
    /// it (shortcut `[label]`, collapsed `[text][]`, and full `[text][label]`
    /// forms) so every one keeps resolving. Uses are recognized everywhere
    /// CommonMark/GFM itself resolves a reference: paragraph and heading
    /// text and GFM table cells, at any container nesting depth (list
    /// items, block quotes). A code span, autolink, raw HTML, or code block
    /// spelling the label is literal text there and is never rewritten.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::Document};
    /// let mut g = Governor::new(Budget::default());
    /// let doc = Document::parse("[tea]\n\n[tea]: /tea\n", &mut g)?;
    /// let plan = doc.relabel_reference(&[2], 0, "chai", &mut g)?;
    /// let applied = plan.apply(doc.source(), Budget::default());
    /// assert_eq!(applied.receipt.evidence.len(), 2); // definition + one use
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects a missing occurrence and an empty/multi-line/bracketed new
    /// label. Rejects a proposal that changes the number of definitions, an
    /// unselected definition, or leaves a use unresolved. Resource
    /// exhaustion and cancellation propagate without a partial plan.
    pub fn relabel_reference(
        &self,
        path: &[usize],
        occurrence: usize,
        new_label: &str,
        governor: &mut Governor,
    ) -> Result<EditPlan, StopReason> {
        governor.charge(Resource::Work, size(self.source.len()).saturating_mul(32))?;
        governor.charge(
            Resource::ScratchBytes,
            size(self.source.len())
                .saturating_mul(128)
                .saturating_add(512),
        )?;
        if new_label.is_empty() || new_label.contains(['[', ']', '\0', '\r', '\n']) {
            return Err(invalid(
                "reference label must be a single non-empty line free of brackets",
            ));
        }
        let sites = self.reference_sites(governor)?;
        let site_index = sites
            .iter()
            .position(|site| site.path == path && site.occurrence == occurrence)
            .ok_or_else(|| invalid("reference definition occurrence does not exist"))?;
        let site = &sites[site_index];
        if site.label == new_label {
            return Ok(EditPlan {
                source: self.digest,
                edits: Vec::new(),
            });
        }
        let label_close = matching_bracket(&self.source, site.span.start)
            .ok_or_else(|| invalid("reference label delimiters are missing"))?;
        let label_range = site.span.start + 1..label_close;
        let refs = document_references(self, governor)?;
        let target = refs
            .definitions()
            .get(site_index)
            .ok_or_else(|| invalid("reference definition index mismatch"))?
            .origin
            .clone();
        let uses = find_uses(self, &target, &refs, governor)?;
        let mut edits = vec![TextEdit {
            range: label_range,
            replacement: new_label.into(),
        }];
        for use_site in &uses {
            let range = use_site
                .label_span
                .clone()
                .filter(|span| !span.is_empty())
                .unwrap_or_else(|| use_site.text_span.clone());
            edits.push(TextEdit {
                range,
                replacement: new_label.into(),
            });
        }
        let plan = EditPlan {
            source: self.digest,
            edits,
        };
        let applied = plan.apply_governed(&self.source, governor);
        let Outcome::Resolved(applied) = applied.outcome else {
            return Err(applied.receipt.stopped);
        };
        let updated = Self::parse(&applied.text, governor)?;
        let updated_sites = updated.reference_sites(governor)?;
        if updated_sites.len() != sites.len() {
            return Err(invalid(
                "relabeling changed the number of reference definitions",
            ));
        }
        for (index, (before, after)) in sites.iter().zip(&updated_sites).enumerate() {
            let expected_label = if index == site_index {
                new_label
            } else {
                before.label.as_str()
            };
            if after.label != expected_label
                || after.destination != before.destination
                || after.title != before.title
            {
                return Err(invalid(
                    "relabeling changed an unselected reference definition",
                ));
            }
        }
        let updated_refs = document_references(&updated, governor)?;
        let updated_target = updated_refs
            .definitions()
            .get(site_index)
            .ok_or_else(|| invalid("relabeling lost the selected reference definition"))?
            .origin
            .clone();
        let resolved = find_uses(&updated, &updated_target, &updated_refs, governor)?;
        if resolved.len() != uses.len() {
            return Err(invalid(
                "relabeling left a use unresolved against its definition",
            ));
        }
        Ok(plan)
    }

    /// Remove a reference definition, inlining its destination and title
    /// into every use that resolves to it. Lossless: every use keeps its
    /// exact visible text and gains an equivalent inline destination.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, Outcome, markdown::Document};
    /// let mut g = Governor::new(Budget::default());
    /// let source = "[tea]\n\n[tea]: /tea\n";
    /// let doc = Document::parse(source, &mut g)?;
    /// let plan = doc.remove_reference(&[2], 0, &mut g)?;
    /// let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else { panic!() };
    /// assert_eq!(applied.text, "[tea](<\\/tea>)\n\n");
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects a missing occurrence and a proposal that changes the number
    /// of remaining definitions or an unselected one. Resource exhaustion
    /// and cancellation propagate without a partial plan.
    pub fn remove_reference(
        &self,
        path: &[usize],
        occurrence: usize,
        governor: &mut Governor,
    ) -> Result<EditPlan, StopReason> {
        governor.charge(Resource::Work, size(self.source.len()).saturating_mul(32))?;
        governor.charge(
            Resource::ScratchBytes,
            size(self.source.len())
                .saturating_mul(128)
                .saturating_add(512),
        )?;
        let sites = self.reference_sites(governor)?;
        let site_index = sites
            .iter()
            .position(|site| site.path == path && site.occurrence == occurrence)
            .ok_or_else(|| invalid("reference definition occurrence does not exist"))?;
        let site = &sites[site_index];
        let refs = document_references(self, governor)?;
        let target = refs
            .definitions()
            .get(site_index)
            .ok_or_else(|| invalid("reference definition index mismatch"))?
            .origin
            .clone();
        let uses = find_uses(self, &target, &refs, governor)?;
        let mut edits = vec![TextEdit {
            range: site.span.clone(),
            replacement: String::new(),
        }];
        for use_site in &uses {
            let mut replacement = String::new();
            if use_site.image {
                replacement.push('!');
            }
            replacement.push('[');
            replacement.push_str(&self.source[use_site.text_span.clone()]);
            replacement.push_str("](<");
            encode(&site.destination, &mut replacement);
            replacement.push('>');
            if let Some(title) = &site.title {
                replacement.push_str(" \"");
                encode(title, &mut replacement);
                replacement.push('"');
            }
            replacement.push(')');
            edits.push(TextEdit {
                range: use_site.span.clone(),
                replacement,
            });
        }
        let plan = EditPlan {
            source: self.digest,
            edits,
        };
        let applied = plan.apply_governed(&self.source, governor);
        let Outcome::Resolved(applied) = applied.outcome else {
            return Err(applied.receipt.stopped);
        };
        let updated = Self::parse(&applied.text, governor)?;
        let updated_sites = updated.reference_sites(governor)?;
        check_remaining_definitions(&sites, site_index, &updated_sites, "removal")?;
        Ok(plan)
    }

    /// Remove a reference definition and turn every use that resolves to it
    /// into plain text (its exact visible text, unbracketed). Each affected
    /// use and the removed definition are recorded as ordinary [`Loss`]
    /// entries when the plan is applied.
    ///
    /// [`Loss`]: crate::contract::Loss
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, Outcome, markdown::Document};
    /// let mut g = Governor::new(Budget::default());
    /// let source = "[tea]\n\n[tea]: /tea\n";
    /// let doc = Document::parse(source, &mut g)?;
    /// let plan = doc.unlink_reference(&[2], 0, &mut g)?;
    /// let Outcome::Resolved(applied) = plan.apply(source, Budget::default()).outcome else { panic!() };
    /// assert_eq!(applied.text, "tea\n\n");
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects a missing occurrence and a proposal that changes the number
    /// of remaining definitions or an unselected one. Resource exhaustion
    /// and cancellation propagate without a partial plan.
    pub fn unlink_reference(
        &self,
        path: &[usize],
        occurrence: usize,
        governor: &mut Governor,
    ) -> Result<EditPlan, StopReason> {
        governor.charge(Resource::Work, size(self.source.len()).saturating_mul(32))?;
        governor.charge(
            Resource::ScratchBytes,
            size(self.source.len())
                .saturating_mul(128)
                .saturating_add(512),
        )?;
        let sites = self.reference_sites(governor)?;
        let site_index = sites
            .iter()
            .position(|site| site.path == path && site.occurrence == occurrence)
            .ok_or_else(|| invalid("reference definition occurrence does not exist"))?;
        let site = &sites[site_index];
        let refs = document_references(self, governor)?;
        let target = refs
            .definitions()
            .get(site_index)
            .ok_or_else(|| invalid("reference definition index mismatch"))?
            .origin
            .clone();
        let uses = find_uses(self, &target, &refs, governor)?;
        let mut edits = vec![TextEdit {
            range: site.span.clone(),
            replacement: String::new(),
        }];
        for use_site in &uses {
            edits.push(TextEdit {
                range: use_site.span.clone(),
                replacement: self.source[use_site.text_span.clone()].into(),
            });
        }
        let plan = EditPlan {
            source: self.digest,
            edits,
        };
        let applied = plan.apply_governed(&self.source, governor);
        let Outcome::Resolved(applied) = applied.outcome else {
            return Err(applied.receipt.stopped);
        };
        let updated = Self::parse(&applied.text, governor)?;
        let updated_sites = updated.reference_sites(governor)?;
        check_remaining_definitions(&sites, site_index, &updated_sites, "unlinking")?;
        Ok(plan)
    }
}
fn check_remaining_definitions(
    sites: &[ReferenceSite],
    removed_index: usize,
    updated_sites: &[ReferenceSite],
    operation: &str,
) -> Result<(), StopReason> {
    if updated_sites.len() != sites.len() - 1 {
        return Err(invalid(match operation {
            "removal" => "removal did not remove exactly one reference definition",
            _ => "unlinking did not remove exactly one reference definition",
        }));
    }
    let remaining = sites
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != removed_index);
    for ((_, before), after) in remaining.zip(updated_sites) {
        if before.label != after.label
            || before.destination != after.destination
            || before.title != after.title
        {
            return Err(invalid(
                "this operation changed an unselected reference definition",
            ));
        }
    }
    Ok(())
}

/// A `[label]`/`![label]`, `[text][]`/`![text][]`, or `[text][label]`/
/// `![text][label]` use resolving to one specific reference definition.
struct UseSite {
    /// Whole use, including any leading `!` for an image.
    span: Range<usize>,
    /// Visible text only (inside the first bracket pair).
    text_span: Range<usize>,
    /// Whether this use is an image.
    image: bool,
    /// The second bracket pair's interior, when present (`Some(empty)` for
    /// a collapsed reference, `Some(nonempty)` for a full reference).
    label_span: Option<Range<usize>>,
}
/// Build one [`References`] view of every reference-definition block in the
/// document, in document order, matching how [`Document::render_html`]
/// resolves reference links/images (see `render::collect`), so a definition
/// found by [`Document::reference_sites`] and a use resolved by parsing
/// ordinary block text against this same collection share content-determined
/// (digest, span) origins that can be compared directly.
fn document_references(doc: &Document, governor: &mut Governor) -> Result<References, StopReason> {
    let mut texts = Vec::new();
    collect_definition_texts(&doc.source, &doc.blocks, &mut texts);
    let sources: Vec<&str> = texts.iter().map(String::as_str).collect();
    References::parse_blocks(&sources, governor)
}
fn collect_definition_texts(source: &str, blocks: &[Block], out: &mut Vec<String>) {
    for block in blocks {
        if matches!(block.kind, BlockKind::ReferenceDefinition) {
            out.push(View::new(source, block).text);
        }
        collect_definition_texts(source, &block.children, out);
    }
}
/// Find every reference-link/image use, in paragraph and heading text
/// anywhere in the document, resolving to `target`'s exact (digest, span)
/// definition origin.
fn find_uses(
    doc: &Document,
    target: &super::ReferenceOrigin,
    refs: &References,
    governor: &mut Governor,
) -> Result<Vec<UseSite>, StopReason> {
    let mut uses = Vec::new();
    collect_uses(doc, &doc.blocks, target, refs, governor, &mut uses)?;
    Ok(uses)
}
fn collect_uses(
    doc: &Document,
    blocks: &[Block],
    target: &super::ReferenceOrigin,
    refs: &References,
    governor: &mut Governor,
    out: &mut Vec<UseSite>,
) -> Result<(), StopReason> {
    for block in blocks {
        governor.charge(Resource::Work, 1)?;
        if matches!(block.kind, BlockKind::Paragraph | BlockKind::Heading { .. }) {
            let view = View::new(&doc.source, block);
            let fragment = InlineFragment::parse_with_references(
                &view.text,
                Dialect::CommonMark,
                refs,
                governor,
            )?;
            let map = |range: &Range<usize>| view.range(range);
            scan_tokens(&view.text, &map, fragment.tokens(), target, out);
        } else if let BlockKind::Table { header, rows, .. } = &block.kind {
            collect_table_cell_uses(
                doc,
                block,
                header.len(),
                rows.len(),
                target,
                refs,
                governor,
                out,
            )?;
        }
        collect_uses(doc, &block.children, target, refs, governor, out)?;
    }
    Ok(())
}
/// Scan every explicitly present cell of a GFM table (header and body rows,
/// never the alignment delimiter) for reference-link/image uses. Each cell
/// is a single physical-line substring (never multi-line), so its absolute
/// position is a plain offset from its own source line's start -- no
/// [`View`] padding reconstruction is needed here.
#[expect(
    clippy::too_many_arguments,
    reason = "each parameter is an independent fact this one governed table-cell scan needs; \
              grouping them into a struct would only move the same eight facts, not reduce them"
)]
fn collect_table_cell_uses(
    doc: &Document,
    block: &Block,
    width: usize,
    row_count: usize,
    target: &super::ReferenceOrigin,
    refs: &References,
    governor: &mut Governor,
    out: &mut Vec<UseSite>,
) -> Result<(), StopReason> {
    for row in 0..=row_count {
        let line_index = if row == 0 { 0 } else { row + 1 };
        let Some(source_line) = block.lines.get(line_index) else {
            continue;
        };
        let text = line(&doc.source, source_line);
        governor.charge(Resource::Work, size(text.len()).saturating_mul(4))?;
        governor.charge(
            Resource::ScratchBytes,
            size(text.len()).saturating_mul(32).saturating_add(256),
        )?;
        for cell in cell_ranges(text).into_iter().take(width) {
            let cell_text = &text[cell.clone()];
            let base = source_line.start + cell.start;
            let fragment =
                InlineFragment::parse_with_references(cell_text, Dialect::Gfm, refs, governor)?;
            let map = |range: &Range<usize>| base + range.start..base + range.end;
            scan_tokens(cell_text, &map, fragment.tokens(), target, out);
        }
    }
    Ok(())
}
fn scan_tokens(
    text: &str,
    map: &dyn Fn(&Range<usize>) -> Range<usize>,
    tokens: &[Inline],
    target: &super::ReferenceOrigin,
    out: &mut Vec<UseSite>,
) {
    for token in tokens {
        match &token.kind {
            InlineKind::Link {
                reference,
                children,
                ..
            }
            | InlineKind::Image {
                reference,
                children,
                ..
            } => {
                if let Some(origin) = reference
                    && origin.source == target.source
                    && origin.span == target.span
                    && let Some((text_span, label_span)) = use_parts(text, &token.span)
                {
                    out.push(UseSite {
                        span: map(&token.span),
                        text_span: map(&text_span),
                        image: matches!(token.kind, InlineKind::Image { .. }),
                        label_span: label_span.map(|span| map(&span)),
                    });
                }
                scan_tokens(text, map, children, target, out);
            }
            InlineKind::Emphasis { children, .. } | InlineKind::Strikethrough { children } => {
                scan_tokens(text, map, children, target, out);
            }
            _ => {}
        }
    }
}
/// Split a resolved reference use's own span into its visible-text bracket
/// and, if present, its separate second bracket (`None` for a shortcut use).
fn use_parts(text: &str, span: &Range<usize>) -> Option<(Range<usize>, Option<Range<usize>>)> {
    let bracket_start = if text.as_bytes().get(span.start) == Some(&b'!') {
        span.start + 1
    } else {
        span.start
    };
    let text_close = matching_bracket(text, bracket_start)?;
    let text_span = bracket_start + 1..text_close;
    if text.as_bytes().get(text_close + 1) == Some(&b'[') {
        let label_close = matching_bracket(text, text_close + 1)?;
        Some((text_span, Some(text_close + 2..label_close)))
    } else {
        Some((text_span, None))
    }
}
/// Find the index of the `]` matching the `[` at `source[start]`, treating
/// `\[`/`\]` as literal and nested unescaped pairs as balanced.
fn matching_bracket(source: &str, start: usize) -> Option<usize> {
    if source.as_bytes().get(start) != Some(&b'[') {
        return None;
    }
    let mut depth = 0_i32;
    let mut escaped = false;
    for (offset, byte) in source.as_bytes()[start..].iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match byte {
            b'\\' => escaped = true,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn collect(
    doc: &Document,
    blocks: &[Block],
    path: &mut Vec<usize>,
    result: &mut Vec<ReferenceSite>,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    for (index, block) in blocks.iter().enumerate() {
        governor.charge(Resource::Work, 1)?;
        path.push(index);
        governor.charge(Resource::Depth, size(path.len().saturating_sub(1)))?;
        if matches!(block.kind, BlockKind::ReferenceDefinition) {
            let view = View::new(&doc.source, block);
            let refs = References::parse_blocks(&[&view.text], governor)?;
            for (occurrence, definition) in refs.definitions().iter().enumerate() {
                governor.charge(
                    Resource::ScratchBytes,
                    size(path.len()).saturating_mul(8).saturating_add(256),
                )?;
                result.push(ReferenceSite {
                    source: doc.digest,
                    path: path.clone(),
                    occurrence,
                    span: view.range(&definition.origin.span),
                    label: definition.label.clone(),
                    destination: definition.destination.clone(),
                    destination_span: view.range(&definition.destination_span),
                    title: definition.title.clone(),
                    title_span: definition.title_span.as_ref().map(|span| view.range(span)),
                });
            }
        }
        collect(doc, &block.children, path, result, governor)?;
        path.pop();
    }
    Ok(())
}

struct View {
    text: String,
    starts: Vec<usize>,
    ends: Vec<usize>,
}
impl View {
    fn new(source: &str, block: &Block) -> Self {
        let mut view = Self {
            text: String::new(),
            starts: Vec::new(),
            ends: Vec::new(),
        };
        for (range, padding) in block.lines.iter().zip(&block.line_padding) {
            view.text.extend(std::iter::repeat_n(' ', *padding));
            view.starts
                .extend(std::iter::repeat_n(range.start, *padding));
            view.ends.extend(std::iter::repeat_n(range.start, *padding));
            view.text.push_str(&source[range.clone()]);
            view.starts.extend(range.clone());
            view.ends.extend(range.start + 1..=range.end);
        }
        view
    }
    fn range(&self, range: &Range<usize>) -> Range<usize> {
        self.starts[range.start]..self.ends[range.end - 1]
    }
}
fn encode(value: &str, result: &mut String) {
    for character in value.chars() {
        match character {
            '\n' => result.push_str("&#10;"),
            '\r' => result.push_str("&#13;"),
            '\t' => result.push_str("&#9;"),
            c if c.is_ascii_punctuation() => {
                result.push('\\');
                result.push(c);
            }
            c => result.push(c),
        }
    }
}
fn invalid(message: &str) -> StopReason {
    StopReason::InvalidInput(message.into())
}
