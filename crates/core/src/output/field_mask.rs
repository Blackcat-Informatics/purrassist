// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The closed context field-mask language, without an evaluator.

use std::fmt;

use crate::contract::{Resource, saturating_size as size};
use crate::{Budget, Governor, ScratchString, ScratchVec, StopReason};

/// A validated context mask. Construction accepts only the four documented forms.
///
/// Members use ASCII identifiers: a letter or underscore, then letters, digits
/// or underscores. ASCII whitespace is allowed outside a mask and around fields
/// and commas in `{a, b}`. Display emits canonical spacing and retains field order.
/// Quoted keys, indexes, functions and general jq expressions are refused.
///
/// ```
/// use purrassist_core::output::{FieldMask, FieldMaskKind};
/// let mask = FieldMask::parse(" {name, total} ").unwrap();
/// assert_eq!(mask.to_string(), "{name, total}");
/// assert!(matches!(mask.kind(), FieldMaskKind::Fields(names) if names.len() == 2));
/// assert!(FieldMask::parse(".name | length").is_err());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldMask {
    kind: Mask,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Mask {
    Path(Vec<String>),
    Elements,
    Fields(Vec<String>),
    OptionalElements(String),
}

/// Borrowed view of an already validated mask; see [`FieldMask`] for examples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldMaskKind<'a> {
    /// A nonempty member path, such as `.customer.name`.
    Path(&'a [String]),
    /// Every element of the root array: `.[]`.
    Elements,
    /// Nonempty distinct members in caller order: `{name, total}`.
    Fields(&'a [String]),
    /// Optional elements of one root member: `.orders[]?`.
    OptionalElements(&'a str),
}

/// A rejected mask, with a byte offset or the explicit governor stop reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldMaskError {
    /// Unsupported syntax, including incomplete or trailing input.
    Unsupported {
        /// UTF-8 byte boundary at the offending token, or the end of input.
        offset: usize,
    },
    /// A field-set member occurred twice.
    DuplicateField {
        /// Start of the repeated identifier in the original input.
        offset: usize,
    },
    /// Work, input or storage admission failed, or the caller cancelled.
    Resource(StopReason),
}

impl fmt::Display for FieldMaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { offset } => {
                write!(f, "unsupported field-mask syntax at byte {offset}")
            }
            Self::DuplicateField { offset } => {
                write!(f, "duplicate field-mask member at byte {offset}")
            }
            Self::Resource(reason) => write!(f, "field-mask admission stopped: {reason:?}"),
        }
    }
}

impl std::error::Error for FieldMaskError {}

impl From<StopReason> for FieldMaskError {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}

impl FieldMask {
    /// Parse under the core's default resource budget.
    ///
    /// # Errors
    /// Returns [`FieldMaskError`] for unsupported syntax, duplicate fields,
    /// or a resource limit. See the type example for a valid and invalid mask.
    pub fn parse(input: &str) -> Result<Self, FieldMaskError> {
        Self::parse_governed(input, &mut Governor::new(Budget::default()))
    }

    /// Parse with explicit work, input, storage and cancellation admission.
    ///
    /// # Errors
    /// Returns [`FieldMaskError::Resource`] before unadmitted scans or retained
    /// allocations. Syntax errors preserve offsets into the original UTF-8 input.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor};
    /// use purrassist_core::output::FieldMask;
    /// let mut governor = Governor::new(Budget::default());
    /// assert_eq!(FieldMask::parse_governed(".customer.name", &mut governor).unwrap().to_string(), ".customer.name");
    /// assert!(governor.usage().work > 0);
    /// ```
    pub fn parse_governed(input: &str, governor: &mut Governor) -> Result<Self, FieldMaskError> {
        governor.charge(Resource::InputBytes, size(input.len()))?;
        governor.charge(Resource::Work, size(input.len()).saturating_add(1))?;
        let mut parser = Parser {
            input,
            at: 0,
            governor,
        };
        parser.space();
        let kind = if parser.take(b'{') {
            let mut names: ScratchVec<String> = ScratchVec::new(parser.governor);
            loop {
                parser.space();
                let offset = parser.at;
                let name = parser.identifier()?;
                for previous in names.as_slice() {
                    parser.governor.charge(
                        Resource::Work,
                        size(name.len().max(previous.len())).saturating_add(1),
                    )?;
                    if previous == name {
                        return Err(FieldMaskError::DuplicateField { offset });
                    }
                }
                names.push(parser.retain(name)?)?;
                parser.space();
                if parser.take(b'}') {
                    break;
                }
                if !parser.take(b',') {
                    return Err(parser.unsupported());
                }
            }
            Mask::Fields(names.into_vec())
        } else if parser.take(b'.') {
            if parser.take(b'[') {
                if !parser.take(b']') {
                    return Err(parser.unsupported());
                }
                Mask::Elements
            } else {
                let first = parser.identifier()?;
                if parser.take(b'[') {
                    if !parser.take(b']') || !parser.take(b'?') {
                        return Err(parser.unsupported());
                    }
                    Mask::OptionalElements(parser.retain(first)?)
                } else {
                    let mut names = ScratchVec::new(parser.governor);
                    names.push(parser.retain(first)?)?;
                    while parser.take(b'.') {
                        let name = parser.identifier()?;
                        names.push(parser.retain(name)?)?;
                    }
                    Mask::Path(names.into_vec())
                }
            }
        } else {
            return Err(parser.unsupported());
        };
        parser.space();
        if parser.at != input.len() {
            return Err(parser.unsupported());
        }
        parser.governor.check()?;
        Ok(Self { kind })
    }

    /// Render canonical spacing through the existing display implementation.
    ///
    /// # Errors
    /// Returns the original work/storage/cancellation failure without partial text.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor};
    /// use purrassist_core::output::FieldMask;
    /// let mask = FieldMask::parse("{a,b}").unwrap();
    /// assert_eq!(mask.render_governed(&mut Governor::new(Budget::default())).unwrap(), "{a, b}");
    /// ```
    pub fn render_governed(&self, governor: &mut Governor) -> Result<String, FieldMaskError> {
        governor.charge(Resource::Work, 1)?;
        let members = match &self.kind {
            Mask::Path(names) | Mask::Fields(names) => names.len(),
            Mask::Elements | Mask::OptionalElements(_) => 1,
        };
        governor.charge(Resource::Work, size(members))?;
        crate::contract::format_output_governed(format_args!("{self}"), governor)
            .map_err(FieldMaskError::Resource)
    }

    /// Inspect the validated operation; the type example shows field access.
    #[must_use]
    pub fn kind(&self) -> FieldMaskKind<'_> {
        match &self.kind {
            Mask::Path(names) => FieldMaskKind::Path(names),
            Mask::Elements => FieldMaskKind::Elements,
            Mask::Fields(names) => FieldMaskKind::Fields(names),
            Mask::OptionalElements(name) => FieldMaskKind::OptionalElements(name),
        }
    }
}

impl fmt::Display for FieldMask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            Mask::Path(names) => {
                for name in names {
                    write!(f, ".{name}")?;
                }
                Ok(())
            }
            Mask::Elements => f.write_str(".[]"),
            Mask::OptionalElements(name) => write!(f, ".{name}[]?"),
            Mask::Fields(names) => {
                f.write_str("{")?;
                for (index, name) in names.iter().enumerate() {
                    if index != 0 {
                        f.write_str(", ")?;
                    }
                    f.write_str(name)?;
                }
                f.write_str("}")
            }
        }
    }
}

struct Parser<'a, 'g> {
    input: &'a str,
    at: usize,
    governor: &'g mut Governor,
}

impl<'a> Parser<'a, '_> {
    fn take(&mut self, byte: u8) -> bool {
        if self.input.as_bytes().get(self.at) == Some(&byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn space(&mut self) {
        while self
            .input
            .as_bytes()
            .get(self.at)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.at += 1;
        }
    }
    const fn unsupported(&self) -> FieldMaskError {
        FieldMaskError::Unsupported { offset: self.at }
    }
    fn identifier(&mut self) -> Result<&'a str, FieldMaskError> {
        let start = self.at;
        if !self
            .input
            .as_bytes()
            .get(self.at)
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
        {
            return Err(self.unsupported());
        }
        self.at += 1;
        while self
            .input
            .as_bytes()
            .get(self.at)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
        {
            self.at += 1;
        }
        Ok(&self.input[start..self.at])
    }
    fn retain(&mut self, name: &str) -> Result<String, FieldMaskError> {
        self.governor.charge(Resource::Work, size(name.len()))?;
        let mut retained = ScratchString::new(self.governor);
        retained.push_str(name)?;
        Ok(retained.into_string())
    }
}
