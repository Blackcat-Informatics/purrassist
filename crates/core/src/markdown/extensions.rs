// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Explicit extension profiles, including upstream per-example conformance profiles.
/// A GFM syntax or rendering extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extension {
    /// Bare URL, www, and email links in text nodes.
    Autolinks = 1,
    /// Double-tilde strikethrough delimiters.
    Strikethrough = 2,
    /// Pipe tables.
    Tables = 4,
    /// Task checkboxes at the start of list items.
    TaskLists = 8,
    /// Escape the nine GFM-disallowed raw HTML tags; not a general sanitizer.
    TagFilter = 16,
}
/// Compact set of enabled extensions; default disables all five.
///
/// ```
/// use purrassist_core::markdown::{Extension, Extensions};
/// assert!(Extensions::gfm().contains(Extension::Autolinks));
/// assert!(!Extensions::default().contains(Extension::Autolinks));
/// let profile = Extensions::default().with(Extension::Tables);
/// assert!(profile.contains(Extension::Tables));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Extensions(u8);
impl Extensions {
    /// The complete GFM extension profile.
    #[must_use]
    pub const fn gfm() -> Self {
        Self(31)
    }
    /// Add one extension to this profile.
    #[must_use]
    pub const fn with(self, extension: Extension) -> Self {
        Self(self.0 | extension as u8)
    }
    /// Whether this profile enables the extension.
    #[must_use]
    pub const fn contains(self, extension: Extension) -> bool {
        self.0 & extension as u8 != 0
    }
}

pub(super) fn filter(source: &str) -> String {
    let mut output = String::new();
    let mut start = 0;
    for (offset, _) in source.match_indices('<') {
        let tail = &source[offset + 1..];
        let name = tail.strip_prefix('/').unwrap_or(tail);
        let width = name.bytes().take_while(u8::is_ascii_alphanumeric).count();
        if name
            .as_bytes()
            .get(width)
            .is_none_or(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/'))
            && [
                "title",
                "textarea",
                "style",
                "xmp",
                "iframe",
                "noembed",
                "noframes",
                "script",
                "plaintext",
            ]
            .iter()
            .any(|tag| name[..width].eq_ignore_ascii_case(tag))
        {
            output.push_str(&source[start..offset]);
            output.push_str("&lt;");
            start = offset + 1;
        }
    }
    output.push_str(&source[start..]);
    output
}
