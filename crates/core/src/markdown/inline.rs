// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Source-preserving inline parsing foundation; full inline conformance is in progress.
mod angle;
mod emphasis;
mod entities;
mod entity_data;
mod links;
mod parse;
mod references;
mod render;
use crate::{Governor, StopReason, contract::Resource, digest::Digest};
pub use references::{ReferenceDefinition, ReferenceOrigin, References};
use std::ops::Range;

/// Markdown dialect for parsing and HTML-output conventions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dialect {
    /// `CommonMark` 0.31.2, retaining nested strong-emphasis tags.
    #[default]
    CommonMark,
    /// Pinned GFM 0.29.0.gfm.13, suppressing directly nested strong tags.
    /// Extension parsing is still in progress; selecting this does not claim conformance.
    Gfm,
}

/// One inline element with an exact range in the original fragment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inline {
    /// Complete source spelling, including syntax delimiters and consumed whitespace.
    pub span: Range<usize>,
    /// Interpreted content; original bytes remain available from the fragment.
    pub kind: InlineKind,
}
/// Source-addressed inline forms and nested content.
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
    /// Emphasized content; children exclude the consumed delimiter bytes.
    Emphasis {
        /// Two delimiters produce strong emphasis; one produces ordinary emphasis.
        strong: bool,
        /// Nested content with ranges in the original fragment.
        children: Vec<Inline>,
    },
    /// An inline or reference link.
    Link {
        /// Decoded destination before URI serialization.
        destination: String,
        /// Optional decoded title.
        title: Option<String>,
        /// Definition source for a reference link; absent for an inline link.
        reference: Option<ReferenceOrigin>,
        /// Link label with ranges in the original fragment.
        children: Vec<Inline>,
    },
    /// An inline image whose nested label supplies alternative text.
    Image {
        /// Decoded image destination before URI serialization.
        destination: String,
        /// Optional decoded title.
        title: Option<String>,
        /// Definition source for a reference image; absent for an inline image.
        reference: Option<ReferenceOrigin>,
        /// Alternative-label content with original fragment ranges.
        children: Vec<Inline>,
    },
    /// A soft line ending.
    SoftBreak,
    /// A two-space or backslash hard line break.
    HardBreak,
    /// Boundary whitespace that has no rendered content.
    Boundary,
}
/// Exact inline source and a contiguous token index.
///
/// Recognizes code spans, escapes, breaks, autolinks, raw HTML, emphasis, and
/// inline links/images. Reference links use an explicit definition collection.
/// Named and numeric character references are decoded after syntax recognition. This is not a complete renderer.
#[derive(Clone, Debug)]
pub struct InlineFragment {
    source: String,
    digest: Digest,
    tokens: Vec<Inline>,
    dialect: Dialect,
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
        Self::parse_with_dialect(source, Dialect::CommonMark, governor)
    }
    /// Parse with an explicit dialect while retaining the same source-span contract.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::{Dialect, InlineFragment}};
    /// let mut governor = Governor::new(Budget::default());
    /// let fragment = InlineFragment::parse_with_dialect("****tea****", Dialect::Gfm, &mut governor)?;
    /// assert_eq!(fragment.render_html(&mut governor)?, "<strong>tea</strong>");
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Returns resource exhaustion or cancellation before publishing a partial tree.
    pub fn parse_with_dialect(
        source: &str,
        dialect: Dialect,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        Self::parse_with_references(source, dialect, &References::default(), governor)
    }
    /// Parse an inline fragment using document-ordered reference definitions.
    ///
    /// Full, collapsed, and shortcut references resolve only against supplied
    /// definitions. The original fragment and definition origin remain available.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::{Dialect, InlineFragment, References}};
    /// let mut g = Governor::new(Budget::default());
    /// let refs = References::parse_blocks(&["[tea]: /tea\n"], &mut g)?;
    /// let text = InlineFragment::parse_with_references("[Tea]", Dialect::CommonMark, &refs, &mut g)?;
    /// assert_eq!(text.render_html(&mut g)?, "<a href=\"/tea\">Tea</a>");
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Returns explicit resource exhaustion or cancellation, without a partial tree.
    pub fn parse_with_references(
        source: &str,
        dialect: Dialect,
        references: &References,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
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
            tokens: parse::run(source, references, governor)?,
            dialect,
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
        render::tokens(&self.tokens, &mut output, governor, 0, self.dialect, false)?;
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
