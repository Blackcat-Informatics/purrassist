// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Source-retaining reference definitions and Unicode label lookup.
use super::links;
use crate::{Governor, StopReason, contract::Resource, digest::Digest};
use std::{collections::BTreeMap, ops::Range};

/// Source identity and exact definition range supporting a reference link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceOrigin {
    /// Digest of the original paragraph block supplied to the reference parser.
    pub source: Digest,
    /// Definition bytes within that block, including its terminating line ending.
    pub span: Range<usize>,
}
/// A definition occurrence, including duplicate labels that do not win lookup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceDefinition {
    /// Original supplied block index.
    pub block: usize,
    /// Exact source bytes supporting this definition.
    pub origin: ReferenceOrigin,
    /// Label spelling between brackets, before case folding.
    pub label: String,
    /// Destination after escapes and entities are decoded.
    pub destination: String,
    /// Destination syntax, including angle delimiters when present, within the supplied block.
    pub destination_span: Range<usize>,
    /// Optional decoded title.
    pub title: Option<String>,
    /// Title syntax including its delimiters, within the supplied block.
    pub title_span: Option<Range<usize>>,
}
/// Definitions collected from paragraph prefixes in document order.
///
/// Container prefixes and code/HTML blocks must be handled by the block parser
/// before calling this API. Definitions cannot interrupt paragraph text.
/// Duplicate labels are retained; the first definition wins lookup.
///
/// ```
/// use purrassist_core::{Budget, Governor, markdown::References};
/// let mut governor = Governor::new(Budget::default());
/// let refs = References::parse_blocks(&["[Tea]: /tea\n", "[tea]: /other\n"], &mut governor)?;
/// assert_eq!(refs.definitions().len(), 2);
/// assert_eq!(refs.consumed()[0], 12);
/// # Ok::<(), purrassist_core::StopReason>(())
/// ```
#[derive(Clone, Debug, Default)]
pub struct References {
    sources: Vec<String>,
    consumed: Vec<usize>,
    definitions: Vec<ReferenceDefinition>,
    index: BTreeMap<String, usize>,
}
impl References {
    /// Parse consecutive definition prefixes of each supplied paragraph block.
    ///
    /// Retains every supplied block and its consumed prefix length. Invalid
    /// definition syntax remains unconsumed. No partial collection is returned.
    ///
    /// # Errors
    /// Returns explicit resource exhaustion or cancellation.
    pub fn parse_blocks(blocks: &[&str], governor: &mut Governor) -> Result<Self, StopReason> {
        governor.check()?;
        let mut result = Self::default();
        for (block, source) in blocks.iter().enumerate() {
            let size = super::super::size(source.len());
            governor.charge(Resource::InputBytes, size)?;
            governor.charge(
                Resource::ScratchBytes,
                size.saturating_mul(32).saturating_add(256),
            )?;
            governor.charge(Resource::Work, size.saturating_mul(4).saturating_add(1))?;
            let digest = Digest::of(source.as_bytes());
            let mut cursor = 0;
            while let Some(parsed) = definition(source, cursor, governor)? {
                // Reserve definition-vector capacity and label-index metadata
                // separately from retained/decoded text, including new field spans.
                governor.charge(Resource::ScratchBytes, 512)?;
                governor.charge(
                    Resource::Work,
                    super::super::size(parsed.label.len())
                        .saturating_mul(
                            16 + u64::from(
                                super::super::size(result.index.len())
                                    .saturating_add(1)
                                    .ilog2(),
                            ),
                        )
                        .saturating_add(1),
                )?;
                let key = label_key(&parsed.label);
                let id = result.definitions.len();
                result.index.entry(key).or_insert(id);
                result.definitions.push(ReferenceDefinition {
                    block,
                    origin: ReferenceOrigin {
                        source: digest,
                        span: cursor..parsed.end,
                    },
                    label: parsed.label,
                    destination: parsed.destination,
                    destination_span: parsed.destination_span,
                    title: parsed.title,
                    title_span: parsed.title_span,
                });
                cursor = parsed.end;
            }
            result.sources.push((*source).into());
            result.consumed.push(cursor);
        }
        Ok(result)
    }
    /// Exact original blocks in document order.
    #[must_use]
    pub fn sources(&self) -> &[String] {
        &self.sources
    }
    /// Number of leading definition bytes consumed in each original block.
    #[must_use]
    pub fn consumed(&self) -> &[usize] {
        &self.consumed
    }
    /// Every recognized definition, including shadowed duplicates.
    #[must_use]
    pub fn definitions(&self) -> &[ReferenceDefinition] {
        &self.definitions
    }
    pub(super) fn lookup(
        &self,
        label: &str,
        governor: &mut Governor,
    ) -> Result<Option<&ReferenceDefinition>, StopReason> {
        governor.charge(
            Resource::Work,
            super::super::size(label.len())
                .saturating_mul(16)
                .saturating_add(1),
        )?;
        governor.charge(
            Resource::ScratchBytes,
            super::super::size(label.len()).saturating_mul(4),
        )?;
        Ok(self
            .index
            .get(&label_key(label))
            .map(|index| &self.definitions[*index]))
    }
}
fn whitespace(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\r' | '\n' | '\u{c}')
        || crate::text::general_category(character) == "Zs"
}
fn label_key(label: &str) -> String {
    crate::text::case_fold(&label.replace('\0', "\u{fffd}"))
        .split(whitespace)
        .filter(|piece| !piece.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
/// Return the closing bracket offset; no unescaped brackets, at most 999 characters.
pub(super) fn label_end(
    source: &str,
    start: usize,
    governor: &mut Governor,
) -> Result<Option<usize>, StopReason> {
    governor.charge(Resource::Work, 1)?;
    let mut escaped = false;
    let mut count = 0;
    let mut nonspace = false;
    for (offset, character) in source[start..].char_indices() {
        governor.charge(
            Resource::Work,
            super::super::size(character.len_utf8()).saturating_add(1),
        )?;
        if !escaped {
            if character == ']' {
                return Ok((count > 0 && nonspace).then_some(start + offset));
            }
            if character == '[' {
                return Ok(None);
            }
        }
        count += 1;
        if count > 999 {
            return Ok(None);
        }
        nonspace |= !whitespace(character);
        escaped = !escaped && character == '\\';
    }
    Ok(None)
}
struct Definition {
    end: usize,
    label: String,
    destination: String,
    destination_span: Range<usize>,
    title: Option<String>,
    title_span: Option<Range<usize>>,
}
fn definition(
    source: &str,
    start: usize,
    governor: &mut Governor,
) -> Result<Option<Definition>, StopReason> {
    governor.charge(Resource::Work, 1)?;
    let mut indent = 0;
    while indent < 4 && source.as_bytes().get(start + indent) == Some(&b' ') {
        governor.charge(Resource::Work, 1)?;
        indent += 1;
    }
    let opening = start + indent;
    if indent > 3 || source.as_bytes().get(opening) != Some(&b'[') {
        return Ok(None);
    }
    let Some(close) = label_end(source, opening + 1, governor)? else {
        return Ok(None);
    };
    if source.as_bytes().get(close + 1) != Some(&b':') {
        return Ok(None);
    }
    let destination_start = links::spaces(source, close + 2, governor)?;
    if blank_line(&source[close + 2..destination_start], governor)? {
        return Ok(None);
    }
    let Some((end, destination)) = links::destination(source, destination_start, governor)? else {
        return Ok(None);
    };
    if end == destination_start {
        return Ok(None);
    }
    let after = links::spaces(source, end, governor)?;
    let (finish, title_value, title_span) = if after > end
        && !blank_line(&source[end..after], governor)?
        && let Some((title_end, title)) = links::title(source, after, governor)?
        && let Some(end) = line_end(source, title_end, governor)?
    {
        (Some(end), Some(title), Some(after..title_end))
    } else {
        (line_end(source, end, governor)?, None, None)
    };
    let Some(finish) = finish else {
        return Ok(None);
    };
    Ok(Some(Definition {
        end: finish,
        label: source[opening + 1..close].into(),
        destination,
        destination_span: destination_start..end,
        title: title_value,
        title_span,
    }))
}
fn line_end(
    source: &str,
    start: usize,
    governor: &mut Governor,
) -> Result<Option<usize>, StopReason> {
    governor.charge(Resource::Work, 1)?;
    let mut end = start;
    while matches!(source.as_bytes().get(end), Some(b' ' | b'\t')) {
        governor.charge(Resource::Work, 1)?;
        end += 1;
    }
    Ok(match source.as_bytes().get(end) {
        None => Some(end),
        Some(b'\n') => Some(end + 1),
        Some(b'\r') => Some(end + 1 + usize::from(source.as_bytes().get(end + 1) == Some(&b'\n'))),
        _ => None,
    })
}
pub(super) fn blank_line(source: &str, governor: &mut Governor) -> Result<bool, StopReason> {
    governor.charge(Resource::Work, 1)?;
    let mut cursor = 0;
    let mut first = true;
    let mut whitespace = true;
    while cursor < source.len() {
        governor.charge(Resource::Work, 1)?;
        match source.as_bytes()[cursor] {
            b'\r' | b'\n' => {
                if !first && whitespace {
                    return Ok(true);
                }
                first = false;
                whitespace = true;
                if source.as_bytes()[cursor] == b'\r'
                    && source.as_bytes().get(cursor + 1) == Some(&b'\n')
                {
                    governor.charge(Resource::Work, 1)?;
                    cursor += 1;
                }
            }
            b' ' | b'\t' => {}
            _ => whitespace = false,
        }
        cursor += 1;
    }
    Ok(false)
}
