// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! One validated, source-retaining JSON Lines result projection path.

use std::fmt;

use crate::{
    Governor, ScratchCharge, ScratchString, ScratchVec, StopReason,
    contract::{Resource, saturating_size as size},
    json::{self, JsonField, JsonLine, Node, Value},
    schema::{Schema, ValidationReport},
};

use super::{ContextManagement, OutputLimitError, OutputMeasurement};
use super::{FieldMask, FieldMaskKind};

/// The last pipeline stage attempted, retained on every outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionStage {
    /// Validate the complete original result, before applying a mask.
    Validation,
    /// Select the mask's borrowed values and transformations.
    Selection,
    /// Serialize the complete compact JSON Lines stream.
    Serialization,
    /// Admit the complete framed bytes through the canonical limit checker.
    ByteLimit,
    /// All validation, selection, serialization and byte checks passed.
    Complete,
}

/// Why projection selected an explicit null rather than a source value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NullSelection {
    /// The uniquely requested member was absent.
    Missing,
    /// Lookup propagated through a null or previously absent value.
    Propagated,
}

/// An explicit projection transformation, never an inferred source fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputTransform<'a> {
    /// The complete supplied mask was applied, without truncating any value.
    MaskApplied,
    /// One mask segment selected a generated null.
    NullInserted {
        /// Requested member name, borrowed from the original mask.
        field: &'a str,
        /// Zero-based path segment or requested field position.
        index: usize,
        /// Missing versus propagated-null selection.
        cause: NullSelection,
    },
    /// Optional iteration emitted no values from its non-array selection.
    OptionalOmitted {
        /// Requested root member, borrowed from the original mask.
        field: &'a str,
    },
}
impl ScratchCharge for OutputTransform<'_> {
    const CHARGE: usize = 40;
}
const _: () = assert!(core::mem::size_of::<OutputTransform<'_>>() <= OutputTransform::CHARGE);

/// Original input and completed transformation evidence, retained on all outcomes.
///
/// Source DOM/schema/mask preparation is caller-owned. The original is never
/// modified; its duplicate occurrences, number spelling and spans remain intact.
/// This trace establishes no graph admission, authorization or host dispatch.
#[derive(Debug)]
pub struct ProjectionTrace<'a> {
    /// Complete immutable original result, including fields the mask omits.
    pub original: &'a Node,
    /// Original validated field mask.
    pub mask: &'a FieldMask,
    /// Last stage attempted; only successful artifacts reach `Complete`.
    pub stage: ProjectionStage,
    /// Full original-output validation evidence, when validation completed.
    pub validation: Option<ValidationReport>,
    /// Actual mask/null/optional transformations, in operation order.
    pub transformations: ScratchVec<OutputTransform<'a>>,
}

/// A typed atomic refusal; no successful or partial serialized output is present.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectionFailure<'a> {
    /// The complete original result failed its supplied output schema.
    Schema,
    /// A path or iteration encountered an unsupported value type.
    WrongType {
        /// Named lookup, or `None` for root-array iteration.
        field: Option<&'a str>,
    },
    /// A selected member name occurred more than once; no winner was chosen.
    AmbiguousMember {
        /// Exact selected name, borrowed from the mask.
        field: &'a str,
    },
    /// Canonical exact-byte refusal, including its complete measurement.
    OutputLimit(OutputLimitError),
    /// Native governance, allocation or cancellation refused processing.
    Resource(StopReason),
}
impl From<StopReason> for ProjectionFailure<'_> {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}

/// A refusal retaining the original result, original mask and stage trace.
#[derive(Debug)]
pub struct ResultProjectionError<'a> {
    /// Complete original and partial/completed stage evidence.
    pub trace: ProjectionTrace<'a>,
    /// Exact semantic or native resource refusal.
    pub reason: ProjectionFailure<'a>,
}
impl fmt::Display for ResultProjectionError<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "result projection stopped at {:?}: ",
            self.trace.stage
        )?;
        match &self.reason {
            ProjectionFailure::Schema => formatter.write_str("original output failed its schema"),
            ProjectionFailure::WrongType { field } => {
                write!(formatter, "wrong projection type at {field:?}")
            }
            ProjectionFailure::AmbiguousMember { field } => {
                write!(formatter, "ambiguous selected member {field:?}")
            }
            ProjectionFailure::OutputLimit(error) => error.fmt(formatter),
            ProjectionFailure::Resource(reason) => reason.fmt(formatter),
        }
    }
}
impl std::error::Error for ResultProjectionError<'_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.reason {
            ProjectionFailure::OutputLimit(error) => Some(error),
            ProjectionFailure::Resource(reason) => Some(reason),
            _ => None,
        }
    }
}

/// A complete, checked compact JSON Lines artifact with original/trace retention.
///
/// Every emitted JSON value has exactly one trailing LF; zero values emit zero
/// bytes. There is no implicit array wrapper. This is one result-processing
/// operation, not graph authority, a host release receipt or history pruning.
#[derive(Debug)]
pub struct ProjectedResult<'a> {
    /// Complete original and successful pipeline evidence.
    pub trace: ProjectionTrace<'a>,
    /// Complete checked stream; its scratch reservation releases on drop.
    pub bytes: ScratchString,
    /// Exact byte count, unit, enforcement mode and supplied ceiling.
    pub measurement: OutputMeasurement,
}

impl ContextManagement {
    /// Validate, project, serialize and atomically admit one complete result.
    ///
    /// Paths and field objects emit one value, using null for missing/null
    /// lookups. `.[]` iterates a root array; `.name[]?` emits a selected array's
    /// values, or none for a missing/null/non-array selection. Named lookups on
    /// other non-object types fail, and selected duplicate names are ambiguous.
    /// Nested values preserve their original duplicate members and number text.
    ///
    /// All values share one compact JSON Lines representation: exactly one LF
    /// per value and no outer array. The existing byte checker sees the entire
    /// stream once, after serialization; no tokenizer can override its result.
    /// Original input/mask and explicit transformations survive every refusal.
    /// Source/schema/context preparation, history and host authority are separate.
    ///
    /// # Errors
    /// Returns a typed schema, selection, byte-limit or native resource refusal
    /// with the last attempted stage. No successful partial bytes are returned.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, json, schema::Schema};
    /// use purrassist_core::output::{ContextManagement, FieldMask, LoadingStrategy, OutputByteLimit};
    /// let context = ContextManagement::new(FieldMask::parse(".orders[]?").unwrap(), OutputByteLimit::new(32).unwrap(), 0, vec![], LoadingStrategy::Always);
    /// let schema = Schema::compile(r#"{"type":"object"}"#).unwrap();
    /// let original = json::parse(r#"{"orders":[1.00e2,true]}"#).unwrap();
    /// let artifact = context.project_result(&schema, &original, &mut Governor::new(Budget::default())).unwrap();
    /// assert_eq!(artifact.bytes.as_str(), "1.00e2\ntrue\n");
    /// assert!(core::ptr::eq(artifact.trace.original, &raw const original));
    /// ```
    #[allow(
        clippy::result_large_err,
        reason = "resource refusals retain complete validation evidence without allocating an error"
    )]
    pub fn project_result<'a>(
        &'a self,
        schema: &Schema,
        original: &'a Node,
        governor: &mut Governor,
    ) -> Result<ProjectedResult<'a>, ResultProjectionError<'a>> {
        let mut trace = ProjectionTrace {
            original,
            mask: self.output_projection(),
            stage: ProjectionStage::Validation,
            validation: None,
            transformations: ScratchVec::new(governor),
        };
        match project(self, schema, &mut trace, governor) {
            Ok((bytes, measurement)) => Ok(ProjectedResult {
                trace,
                bytes,
                measurement,
            }),
            Err(reason) => Err(ResultProjectionError { trace, reason }),
        }
    }
}

fn project<'a>(
    context: &ContextManagement,
    schema: &Schema,
    trace: &mut ProjectionTrace<'a>,
    governor: &mut Governor,
) -> Result<(ScratchString, OutputMeasurement), ProjectionFailure<'a>> {
    let validation = schema.validate_with(trace.original, governor)?;
    let valid = validation.valid;
    trace.validation = Some(validation);
    if !valid {
        return Err(ProjectionFailure::Schema);
    }
    trace.stage = ProjectionStage::Selection;
    let mut lines = ScratchVec::new(governor);
    let mut fields = ScratchVec::new(governor);
    let object = select(trace, &mut lines, &mut fields, governor)?;
    governor.charge(Resource::Work, 1)?;
    trace.transformations.push(OutputTransform::MaskApplied)?;
    trace.stage = ProjectionStage::Serialization;
    let bytes = if object {
        json::lines_to_json_scratch(&[JsonLine::Object(fields.as_slice())], governor)?
    } else {
        json::lines_to_json_scratch(lines.as_slice(), governor)?
    };
    trace.stage = ProjectionStage::ByteLimit;
    let measurement = context
        .output_limit()
        .check(bytes.as_str().as_bytes(), governor)
        .map_err(|error| match error {
            OutputLimitError::Resource(reason) => ProjectionFailure::Resource(reason),
            error => ProjectionFailure::OutputLimit(error),
        })?;
    governor.check()?;
    trace.stage = ProjectionStage::Complete;
    Ok((bytes, measurement))
}

fn select<'a>(
    trace: &mut ProjectionTrace<'a>,
    lines: &mut ScratchVec<JsonLine<'a>>,
    fields: &mut ScratchVec<JsonField<'a>>,
    governor: &mut Governor,
) -> Result<bool, ProjectionFailure<'a>> {
    governor.charge(Resource::Work, 1)?;
    match trace.mask.kind() {
        FieldMaskKind::Path(path) => {
            let mut selected = Some(trace.original);
            for (index, field) in path.iter().enumerate() {
                selected = lookup(selected, field, index, trace, governor)?;
            }
            lines.push(JsonLine::Value(selected))?;
        }
        FieldMaskKind::Fields(names) => {
            for (index, field) in names.iter().enumerate() {
                let value = lookup(Some(trace.original), field, index, trace, governor)?;
                fields.push(JsonField { key: field, value })?;
            }
            return Ok(true);
        }
        FieldMaskKind::Elements => array_lines(trace.original, None, lines, governor)?,
        FieldMaskKind::OptionalElements(field) => {
            let selected = lookup(Some(trace.original), field, 0, trace, governor)?;
            if let Some(value) = selected.filter(|value| matches!(value.value, Value::Array(_))) {
                array_lines(value, Some(field), lines, governor)?;
            } else {
                governor.charge(Resource::Work, 1)?;
                trace
                    .transformations
                    .push(OutputTransform::OptionalOmitted { field })?;
            }
        }
    }
    Ok(false)
}

fn lookup<'a>(
    source: Option<&'a Node>,
    field: &'a str,
    index: usize,
    trace: &mut ProjectionTrace<'a>,
    governor: &mut Governor,
) -> Result<Option<&'a Node>, ProjectionFailure<'a>> {
    governor.charge(Resource::Work, 1)?;
    let members = match source.map(|node| &node.value) {
        None | Some(Value::Null) => {
            trace.transformations.push(OutputTransform::NullInserted {
                field,
                index,
                cause: NullSelection::Propagated,
            })?;
            return Ok(None);
        }
        Some(Value::Object(members)) => members,
        Some(_) => return Err(ProjectionFailure::WrongType { field: Some(field) }),
    };
    let mut found = None;
    for member in members {
        governor.charge(
            Resource::Work,
            size(member.key.len().min(field.len())).saturating_add(1),
        )?;
        if member.key == field {
            if found.is_some() {
                return Err(ProjectionFailure::AmbiguousMember { field });
            }
            found = Some(&member.value);
        }
    }
    if found.is_none() {
        trace.transformations.push(OutputTransform::NullInserted {
            field,
            index,
            cause: NullSelection::Missing,
        })?;
    }
    Ok(found)
}

fn array_lines<'a>(
    source: &'a Node,
    field: Option<&'a str>,
    lines: &mut ScratchVec<JsonLine<'a>>,
    governor: &mut Governor,
) -> Result<(), ProjectionFailure<'a>> {
    let Value::Array(items) = &source.value else {
        return Err(ProjectionFailure::WrongType { field });
    };
    for item in items {
        governor.charge(Resource::Work, 1)?;
        lines.push(JsonLine::Value(Some(item)))?;
    }
    Ok(())
}
