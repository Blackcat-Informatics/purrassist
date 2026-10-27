// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The complete context fragment over caller-owned JSON values.

use super::{OutputByteLimit, OutputEnforcement};
use crate::{FieldMask, FieldMaskError};
use purrassist_core::{
    Governor, ScratchString, ScratchVec, StopReason,
    contract::{Resource, saturating_size},
    json::{IntegerReading, Member, Node, Number, Value},
};
use std::fmt;

/// Required format of the enclosing bundle; never a context-object field.
pub const MCP_CONTEXT_FORMAT: &str = "purrassist-toolgraph-v2";
const FIELDS: [&str; 6] = [
    "outputProjectionJq",
    "maxOutputBytes",
    "outputEnforcement",
    "historyRetentionTurns",
    "intentTags",
    "loadingStrategy",
];

/// Whether the tool declaration is always loaded or selected on demand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadingStrategy {
    /// Include this tool in every applicable selection.
    Always,
    /// Include this tool only when explicitly selected.
    OnDemand,
}
impl LoadingStrategy {
    const fn text(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::OnDemand => "on_demand",
        }
    }
}

/// A rejected context fragment, distinct from exhausted resources.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContextCodecError {
    /// The enclosing bundle format was not supplied.
    MissingFormat,
    /// The supplied enclosing format is unsupported; no migration is attempted.
    UnsupportedFormat,
    /// An unknown member at this original object position.
    UnknownField {
        /// Original member index.
        index: usize,
    },
    /// A known field was repeated.
    DuplicateField(&'static str),
    /// A required field was absent.
    MissingField(&'static str),
    /// The legacy token-bound field cannot be reinterpreted as bytes.
    LegacyTokenLimit,
    /// The named field has the wrong JSON type.
    WrongType(&'static str),
    /// A quantity is negative, fractional, zero where prohibited, or out of range.
    Quantity(&'static str),
    /// The declared output enforcement is not exact bytes.
    Enforcement,
    /// The loading strategy is not recognized.
    Loading,
    /// The tag at this original array position is not a string.
    TagType {
        /// Original tag index.
        index: usize,
    },
    /// The existing mask parser refused its syntax.
    Projection(FieldMaskError),
    /// Admission, allocation or cancellation refused the operation.
    Resource(StopReason),
}
impl fmt::Display for ContextCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFormat => f.write_str("supply the enclosing tool-graph bundle format"),
            Self::UnsupportedFormat => {
                f.write_str("context requires enclosing purrassist-toolgraph-v2 format")
            }
            Self::UnknownField { index } => write!(f, "unknown context member at index {index}"),
            Self::DuplicateField(field) => write!(f, "duplicate context field {field}"),
            Self::MissingField(field) => write!(f, "missing context field {field}"),
            Self::LegacyTokenLimit => {
                f.write_str("legacy maxOutputTokens cannot be converted to an exact byte limit")
            }
            Self::WrongType(field) => write!(f, "wrong JSON type for context field {field}"),
            Self::Quantity(field) => {
                write!(f, "invalid unsigned quantity in context field {field}")
            }
            Self::Enforcement => f.write_str("outputEnforcement must be exact-bytes"),
            Self::Loading => f.write_str("loadingStrategy must be always or on_demand"),
            Self::TagType { index } => write!(f, "intentTags item {index} must be a string"),
            Self::Projection(error) => error.fmt(f),
            Self::Resource(reason) => reason.fmt(f),
        }
    }
}
impl std::error::Error for ContextCodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Projection(error) => Some(error),
            Self::Resource(reason) => Some(reason),
            _ => None,
        }
    }
}

/// Complete validated context metadata; output enforcement is always exact bytes.
///
/// The caller owns source DOM construction. This component validates and emits
/// one fragment; full manifest, RDF and host-release integration are separate.
/// Tags preserve their values, order and duplicates.
///
/// ```
/// use purrassist_core::{Budget, Governor, json};
/// use purrassist_toolgraph::{ContextManagement, MCP_CONTEXT_FORMAT};
/// let source = json::parse(r#"{"outputProjectionJq":".orders","maxOutputBytes":1e2,"outputEnforcement":"exact-bytes","historyRetentionTurns":0,"intentTags":[],"loadingStrategy":"always"}"#).unwrap();
/// let context = ContextManagement::decode_mcp(Some(MCP_CONTEXT_FORMAT), &source, &mut Governor::new(Budget::default())).unwrap();
/// assert_eq!(context.output_limit().max_bytes(), 100);
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct ContextManagement {
    output_projection: FieldMask,
    output_limit: OutputByteLimit,
    history_retention_turns: u64,
    intent_tags: Vec<String>,
    loading: LoadingStrategy,
}
impl ContextManagement {
    /// Take caller-owned validated components without copying or allocating.
    #[must_use]
    pub const fn new(
        output_projection: FieldMask,
        output_limit: OutputByteLimit,
        history_retention_turns: u64,
        intent_tags: Vec<String>,
        loading: LoadingStrategy,
    ) -> Self {
        Self {
            output_projection,
            output_limit,
            history_retention_turns,
            intent_tags,
            loading,
        }
    }

    /// Decode all six required fields from an immutable caller-owned JSON object.
    ///
    /// # Errors
    /// Rejects invalid version, legacy/unknown/duplicate/missing fields, types,
    /// quantities, modes or masks. Resource failures remain distinct and no
    /// partial successful context is exposed. See the type example.
    ///
    /// # Panics
    /// Panics if the internal field-map invariant is violated after completeness
    /// checking. Missing input fields are rejected before extraction.
    pub fn decode_mcp(
        format: Option<&str>,
        source: &Node,
        governor: &mut Governor,
    ) -> Result<Self, ContextCodecError> {
        version(format, governor)?;
        level(governor, 0)?;
        let Value::Object(members) = &source.value else {
            return Err(ContextCodecError::WrongType("contextManagement"));
        };
        let mut fields = [None; 6];
        for (index, member) in members.iter().enumerate() {
            governor
                .charge(Resource::Work, 1)
                .map_err(ContextCodecError::Resource)?;
            if equal(&member.key, "maxOutputTokens", governor)? {
                return Err(ContextCodecError::LegacyTokenLimit);
            }
            let mut selected = None;
            for (slot, name) in FIELDS.iter().enumerate() {
                if equal(&member.key, name, governor)? {
                    selected = Some(slot);
                    break;
                }
            }
            let Some(selected) = selected else {
                return Err(ContextCodecError::UnknownField { index });
            };
            if fields[selected].is_some() {
                return Err(ContextCodecError::DuplicateField(FIELDS[selected]));
            }
            fields[selected] = Some(&member.value);
        }
        for (index, value) in fields.iter().enumerate() {
            governor
                .charge(Resource::Work, 1)
                .map_err(ContextCodecError::Resource)?;
            if value.is_none() {
                return Err(ContextCodecError::MissingField(FIELDS[index]));
            }
        }
        // The presence loop proves each field exists. This fixed-size map holds
        // references only and never copies source nodes or allocates an index.
        let fields =
            fields.map(|value| value.expect("INVARIANT: all required context fields were checked"));
        let projection = FieldMask::parse_governed(text(fields[0], FIELDS[0], governor)?, governor)
            .map_err(projection_error)?;
        let maximum = quantity(fields[1], FIELDS[1], governor)?;
        let output_limit =
            OutputByteLimit::new(maximum).map_err(|_| ContextCodecError::Quantity(FIELDS[1]))?;
        if !equal(
            text(fields[2], FIELDS[2], governor)?,
            "exact-bytes",
            governor,
        )? {
            return Err(ContextCodecError::Enforcement);
        }
        let history = quantity(fields[3], FIELDS[3], governor)?;
        level(governor, 1)?;
        let Value::Array(tags) = &fields[4].value else {
            return Err(ContextCodecError::WrongType(FIELDS[4]));
        };
        let mut retained = ScratchVec::new(governor);
        for (index, tag) in tags.iter().enumerate() {
            level(governor, 2)?;
            let Value::String(tag) = &tag.value else {
                return Err(ContextCodecError::TagType { index });
            };
            retained
                .push(copy(tag, governor)?)
                .map_err(ContextCodecError::Resource)?;
        }
        let loading = text(fields[5], FIELDS[5], governor)?;
        let loading = if equal(loading, "always", governor)? {
            LoadingStrategy::Always
        } else if equal(loading, "on_demand", governor)? {
            LoadingStrategy::OnDemand
        } else {
            return Err(ContextCodecError::Loading);
        };
        governor.check().map_err(ContextCodecError::Resource)?;
        Ok(Self::new(
            projection,
            output_limit,
            history,
            retained.into_vec(),
            loading,
        ))
    }

    /// Encode a canonical six-field DOM under the explicit enclosing v2 format.
    ///
    /// # Errors
    /// Returns version or resource/cancellation failure without partial success.
    /// Source components are never modified. The JSON encoder below uses this
    /// exact DOM path rather than a second wire implementation.
    pub fn encode_mcp(
        &self,
        format: Option<&str>,
        governor: &mut Governor,
    ) -> Result<Node, ContextCodecError> {
        version(format, governor)?;
        level(governor, 0)?;
        let mut members = ScratchVec::new(governor);
        level(governor, 1)?;
        let projection = self
            .output_projection
            .render_governed(governor)
            .map_err(projection_error)?;
        member(&mut members, FIELDS[0], Value::String(projection), governor)?;
        level(governor, 1)?;
        member(
            &mut members,
            FIELDS[1],
            Value::Number(
                Number::from_unsigned_governed(self.output_limit.max_bytes(), governor)
                    .map_err(ContextCodecError::Resource)?,
            ),
            governor,
        )?;
        level(governor, 1)?;
        member(
            &mut members,
            FIELDS[2],
            Value::String(copy("exact-bytes", governor)?),
            governor,
        )?;
        level(governor, 1)?;
        member(
            &mut members,
            FIELDS[3],
            Value::Number(
                Number::from_unsigned_governed(self.history_retention_turns, governor)
                    .map_err(ContextCodecError::Resource)?,
            ),
            governor,
        )?;
        level(governor, 1)?;
        let mut tags = ScratchVec::new(governor);
        for tag in &self.intent_tags {
            level(governor, 2)?;
            tags.push(Node::new(Value::String(copy(tag, governor)?)))
                .map_err(ContextCodecError::Resource)?;
        }
        member(
            &mut members,
            FIELDS[4],
            Value::Array(tags.into_vec()),
            governor,
        )?;
        level(governor, 1)?;
        member(
            &mut members,
            FIELDS[5],
            Value::String(copy(self.loading.text(), governor)?),
            governor,
        )?;
        governor.check().map_err(ContextCodecError::Resource)?;
        Ok(Node::new(Value::Object(members.into_vec())))
    }

    /// Encode through the canonical DOM path and the shared governed JSON writer.
    ///
    /// # Errors
    /// Returns the exact version/resource/cancellation failure; no partial JSON.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor};
    /// use purrassist_toolgraph::{ContextManagement, FieldMask, OutputByteLimit, LoadingStrategy, MCP_CONTEXT_FORMAT};
    /// let context = ContextManagement::new(FieldMask::parse(".orders").unwrap(), OutputByteLimit::new(100).unwrap(), 0, vec![], LoadingStrategy::Always);
    /// assert!(context.encode_json(Some(MCP_CONTEXT_FORMAT), &mut Governor::new(Budget::default())).unwrap().contains(r#""outputEnforcement":"exact-bytes""#));
    /// ```
    pub fn encode_json(
        &self,
        format: Option<&str>,
        governor: &mut Governor,
    ) -> Result<String, ContextCodecError> {
        self.encode_mcp(format, governor)?
            .to_json_governed(governor)
            .map_err(ContextCodecError::Resource)
    }

    /// The validated field-mask operation, without re-parsing.
    #[must_use]
    pub const fn output_projection(&self) -> &FieldMask {
        &self.output_projection
    }
    /// The positive exact byte limit; no token conversion occurs.
    #[must_use]
    pub const fn output_limit(&self) -> OutputByteLimit {
        self.output_limit
    }
    /// The only supported output enforcement mode.
    #[must_use]
    pub const fn output_enforcement(&self) -> OutputEnforcement {
        OutputEnforcement::ExactBytes
    }
    /// The exact number of history turns retained.
    #[must_use]
    pub const fn history_retention_turns(&self) -> u64 {
        self.history_retention_turns
    }
    /// Original tag values in caller order, including duplicates.
    #[must_use]
    pub fn intent_tags(&self) -> &[String] {
        &self.intent_tags
    }
    /// The validated loading strategy.
    #[must_use]
    pub const fn loading(&self) -> LoadingStrategy {
        self.loading
    }
}

fn version(format: Option<&str>, governor: &mut Governor) -> Result<(), ContextCodecError> {
    governor
        .charge(Resource::Work, 1)
        .map_err(ContextCodecError::Resource)?;
    let Some(format) = format else {
        return Err(ContextCodecError::MissingFormat);
    };
    if equal(format, MCP_CONTEXT_FORMAT, governor)? {
        Ok(())
    } else {
        Err(ContextCodecError::UnsupportedFormat)
    }
}
fn level(governor: &mut Governor, depth: u64) -> Result<(), ContextCodecError> {
    governor
        .charge(Resource::Depth, depth)
        .map_err(ContextCodecError::Resource)?;
    governor
        .charge(Resource::Work, 1)
        .map_err(ContextCodecError::Resource)
}
fn equal(left: &str, right: &str, governor: &mut Governor) -> Result<bool, ContextCodecError> {
    governor
        .charge(
            Resource::Work,
            saturating_size(left.len().min(right.len())).saturating_add(1),
        )
        .map_err(ContextCodecError::Resource)?;
    Ok(left == right)
}
fn text<'a>(
    node: &'a Node,
    field: &'static str,
    governor: &mut Governor,
) -> Result<&'a str, ContextCodecError> {
    level(governor, 1)?;
    match &node.value {
        Value::String(text) => Ok(text),
        _ => Err(ContextCodecError::WrongType(field)),
    }
}
fn quantity(
    node: &Node,
    field: &'static str,
    governor: &mut Governor,
) -> Result<u64, ContextCodecError> {
    level(governor, 1)?;
    let Value::Number(number) = &node.value else {
        return Err(ContextCodecError::WrongType(field));
    };
    match number
        .integer_magnitude_governed(governor)
        .map_err(ContextCodecError::Resource)?
    {
        IntegerReading::Exact {
            negative: false,
            magnitude,
        } => u64::try_from(magnitude).map_err(|_| ContextCodecError::Quantity(field)),
        _ => Err(ContextCodecError::Quantity(field)),
    }
}
fn copy(text: &str, governor: &mut Governor) -> Result<String, ContextCodecError> {
    governor
        .charge(Resource::Work, saturating_size(text.len()))
        .map_err(ContextCodecError::Resource)?;
    let mut output = ScratchString::new(governor);
    output.push_str(text).map_err(ContextCodecError::Resource)?;
    Ok(output.into_string())
}
fn member(
    members: &mut ScratchVec<Member>,
    key: &str,
    value: Value,
    governor: &mut Governor,
) -> Result<(), ContextCodecError> {
    members
        .push(Member {
            key: copy(key, governor)?,
            key_span: None,
            value: Node::new(value),
        })
        .map_err(ContextCodecError::Resource)
}
fn projection_error(error: FieldMaskError) -> ContextCodecError {
    match error {
        FieldMaskError::Resource(reason) => ContextCodecError::Resource(reason),
        other => ContextCodecError::Projection(other),
    }
}
