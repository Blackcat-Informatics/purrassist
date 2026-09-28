// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Document-addressed reference definitions and verified destination/title edits.
use super::{Block, BlockKind, Document, References, size};
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
