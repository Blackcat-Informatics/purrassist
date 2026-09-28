// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Source-preserving inline parsing foundation; full inline conformance is in progress.
mod angle;
mod parse;
use crate::{Governor, StopReason, contract::Resource, digest::Digest};
use std::ops::Range;

/// One inline element with an exact range in the original fragment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inline {
    /// Complete source spelling, including syntax delimiters and consumed whitespace.
    pub span: Range<usize>,
    /// Interpreted content; original bytes remain available from the fragment.
    pub kind: InlineKind,
}
/// Inline forms currently recognized before emphasis and explicit-link parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineKind {
    /// Literal text with backslash escapes interpreted.
    Text(String),
    /// Code contents with line endings and outer spaces normalized.
    Code(String),
    /// Angle-delimited URI or email address.
    Autolink {
        /// URI spelling, with `mailto:` supplied for email addresses.
        destination: String,
        /// Visible label, unchanged except line-ending normalization.
        label: String,
    },
    /// Raw inline HTML, with line endings normalized.
    Html(String),
    /// A soft line ending.
    SoftBreak,
    /// A two-space or backslash hard line break.
    HardBreak,
    /// Boundary whitespace that has no rendered content.
    Boundary,
}
/// Exact inline source and a contiguous token index.
///
/// Recognizes code spans, backslash escapes, line breaks, autolinks, and raw HTML.
/// Emphasis, explicit links/images, and entity decoding remain to be implemented;
/// their syntax currently remains literal text. This is not a complete renderer.
#[derive(Clone, Debug)]
pub struct InlineFragment {
    source: String,
    digest: Digest,
    tokens: Vec<Inline>,
}
impl InlineFragment {
    /// Parse the contents of one paragraph or heading, preserving original bytes.
    ///
    /// Block delimiters and container prefixes must already have been removed.
    /// Boundary whitespace is indexed but not rendered. Nested code/HTML/autolink
    /// contents are protected from escape and line-break interpretation.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::InlineFragment};
    /// let mut governor = Governor::new(Budget::default());
    /// let fragment = InlineFragment::parse("Tea `hot`  \r\ncooled", &mut governor)?;
    /// assert_eq!(fragment.render_html(&mut governor)?, "Tea <code>hot</code><br />\ncooled");
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Returns explicit admission, work, or storage exhaustion and cancellation.
    pub fn parse(source: &str, governor: &mut Governor) -> Result<Self, StopReason> {
        governor.charge(Resource::InputBytes, super::size(source.len()))?;
        governor.charge(Resource::Work, super::size(source.len()).saturating_mul(4))?;
        governor.charge(
            Resource::ScratchBytes,
            super::size(source.len())
                .saturating_mul(256)
                .saturating_add(256),
        )?;
        Ok(Self {
            source: source.into(),
            digest: Digest::of(source.as_bytes()),
            tokens: parse::run(source, governor)?,
        })
    }
    /// Exact source before inline interpretation.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
    /// Identity binding every token range to this fragment's exact bytes.
    #[must_use]
    pub const fn digest(&self) -> Digest {
        self.digest
    }
    /// Tokens partitioning the source without overlaps, gaps, or normalized offsets.
    #[must_use]
    pub fn tokens(&self) -> &[Inline] {
        &self.tokens
    }
    /// Render recognized inline forms without paragraph or heading wrappers.
    ///
    /// Raw HTML and URI schemes are preserved; this method is not an HTML sanitizer.
    /// Text, code, labels, and destination attributes receive HTML escaping.
    ///
    /// # Errors
    /// Returns cancellation or work/storage exhaustion before exposing any output.
    pub fn render_html(&self, governor: &mut Governor) -> Result<String, StopReason> {
        governor.charge(
            Resource::Work,
            super::size(self.source.len())
                .saturating_mul(12)
                .saturating_add(1),
        )?;
        governor.charge(
            Resource::ScratchBytes,
            super::size(self.source.len())
                .saturating_mul(16)
                .saturating_add(256),
        )?;
        let mut output = String::new();
        for token in &self.tokens {
            match &token.kind {
                InlineKind::Text(text) => escape(text, &mut output),
                InlineKind::Code(text) => {
                    output.push_str("<code>");
                    escape(text, &mut output);
                    output.push_str("</code>");
                }
                InlineKind::Html(text) => output.push_str(text),
                InlineKind::Autolink { destination, label } => {
                    output.push_str("<a href=\"");
                    escape(&angle::uri(destination), &mut output);
                    output.push_str("\">");
                    escape(label, &mut output);
                    output.push_str("</a>");
                }
                InlineKind::SoftBreak => output.push('\n'),
                InlineKind::HardBreak => output.push_str("<br />\n"),
                InlineKind::Boundary => {}
            }
        }
        Ok(output)
    }
}
fn escape(text: &str, output: &mut String) {
    for character in text.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\0' => output.push('\u{fffd}'),
            _ => output.push(character),
        }
    }
}
fn normalize_lines(source: &str) -> String {
    source
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\0', "\u{fffd}")
}
