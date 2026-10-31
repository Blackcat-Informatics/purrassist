// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Complete borrowed execution-profile wire values, independent of graph agreement.

use purrassist_core::{
    Governor,
    contract::Resource,
    json::{self, IntegerReading, JsonField, JsonLine, Node, Value},
};

use super::{EnvelopeError, EnvelopeProblem, error, fields, step, text};
use crate::ResourceLimits;

const FIELDS: [&str; 10] = [
    "graphBinding",
    "mode",
    "profileName",
    "hatName",
    "egressAllowed",
    "addressSpaceBytes",
    "fileSizeBytes",
    "processCount",
    "cpuSeconds",
    "openFileCount",
];

/// The explicitly selected execution mode; neither implies host availability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Select the named profile through `aa-exec`.
    AaExec,
    /// Select the named hat through `change_hat`.
    ChangeHat,
}

/// Why a JSON number cannot represent an execution-profile wire quantity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionQuantityProblem {
    /// Zero or a negative integer; all five quantities must be positive.
    NonPositive,
    /// The exact mathematical value has a fractional part.
    Fractional,
    /// The exact integer exceeds the wire API's `u64` representation.
    OutOfRange,
}

/// Complete execution-profile fragment borrowing the caller's unchanged JSON.
///
/// Quantities are exact positive `u64` values, including integral decimal and
/// exponent spellings. This wire domain is independent of the narrower limits
/// of [`ResourceLimits::render`]. Profile and hat strings are retained verbatim;
/// this operation proves neither RDF agreement nor permission to execute.
///
/// ```
/// use purrassist_core::{Budget, Governor, json};
/// use purrassist_toolgraph::codec::mcp::{ExecutionMode, ExecutionProfile};
/// let source = json::parse(r#"{"graphBinding":"https://example.org/profile","mode":"aa-exec","profileName":"tool","hatName":"hat","egressAllowed":false,"addressSpaceBytes":1e2,"fileSizeBytes":1,"processCount":1,"cpuSeconds":1,"openFileCount":1}"#).unwrap();
/// let mut governor = Governor::new(Budget::default());
/// let profile = ExecutionProfile::read(&source, &mut governor).unwrap();
/// assert_eq!(profile.mode(), ExecutionMode::AaExec);
/// assert_eq!(profile.limits().address_space_bytes, 100);
/// assert_eq!(profile.encode_json(&mut governor).unwrap(), source.to_json());
/// ```
#[derive(Clone, Copy, Debug)]
pub struct ExecutionProfile<'a> {
    original: &'a Node,
    fields: [&'a Node; 10],
    graph_binding: &'a str,
    mode: ExecutionMode,
    profile_name: &'a str,
    hat_name: &'a str,
    egress_allowed: bool,
    limits: ResourceLimits,
}

impl<'a> ExecutionProfile<'a> {
    /// Decode every field without copying strings, numbers or source locations.
    ///
    /// # Errors
    /// Returns located missing, duplicate, unknown, type, mode or numeric errors,
    /// or the original resource/cancellation refusal. No partial view escapes.
    /// See [`Self`] for a complete round trip.
    pub fn read(source: &'a Node, governor: &mut Governor) -> Result<Self, EnvelopeError> {
        Self::read_at(source, 0, governor)
    }

    pub(super) fn read_at(
        source: &'a Node,
        depth: u64,
        governor: &mut Governor,
    ) -> Result<Self, EnvelopeError> {
        let selected = fields(source, "executionProfile", FIELDS, depth, governor)?;
        for value in selected {
            step(value, Resource::Depth, depth + 1, governor)?;
        }
        let graph_binding = text(selected[0], FIELDS[0], governor)?;
        let mode = text(selected[1], FIELDS[1], governor)?;
        step(selected[1], Resource::Work, 20, governor)?;
        let mode = match mode {
            "aa-exec" => ExecutionMode::AaExec,
            "change_hat" => ExecutionMode::ChangeHat,
            _ => {
                return Err(error(
                    selected[1],
                    EnvelopeProblem::UnsupportedExecutionMode,
                ));
            }
        };
        let profile_name = text(selected[2], FIELDS[2], governor)?;
        let hat_name = text(selected[3], FIELDS[3], governor)?;
        step(selected[4], Resource::Work, 1, governor)?;
        let Value::Bool(egress_allowed) = &selected[4].value else {
            return Err(error(selected[4], EnvelopeProblem::WrongType(FIELDS[4])));
        };
        let limits = ResourceLimits {
            address_space_bytes: quantity(selected[5], FIELDS[5], governor)?,
            file_size_bytes: quantity(selected[6], FIELDS[6], governor)?,
            processes: quantity(selected[7], FIELDS[7], governor)?,
            cpu_seconds: quantity(selected[8], FIELDS[8], governor)?,
            open_files: quantity(selected[9], FIELDS[9], governor)?,
        };
        step(source, Resource::Work, 1, governor)?;
        Ok(Self {
            original: source,
            fields: selected,
            graph_binding,
            mode,
            profile_name,
            hat_name,
            egress_allowed: *egress_allowed,
            limits,
        })
    }

    /// Original JSON, including all member/key spans and scalar spellings.
    #[must_use]
    pub const fn original(&self) -> &'a Node {
        self.original
    }

    /// Unmodified graph reference; resolution belongs to graph admission.
    #[must_use]
    pub const fn graph_binding(&self) -> &'a str {
        self.graph_binding
    }

    /// Explicit wire mode, without fallback or host-availability inference.
    #[must_use]
    pub const fn mode(&self) -> ExecutionMode {
        self.mode
    }

    /// Unmodified profile string; no policy-word or host-label normalization.
    #[must_use]
    pub const fn profile_name(&self) -> &'a str {
        self.profile_name
    }

    /// Unmodified hat string, also retained when the mode is `aa-exec`.
    #[must_use]
    pub const fn hat_name(&self) -> &'a str {
        self.hat_name
    }

    /// Declared egress flag, without evaluating the graph's rules.
    #[must_use]
    pub const fn egress_allowed(&self) -> bool {
        self.egress_allowed
    }

    /// Exact positive wire quantities. Rendering may separately refuse a value
    /// unsupported by its selected native representation; decoding never clamps it.
    #[must_use]
    pub const fn limits(&self) -> ResourceLimits {
        self.limits
    }

    /// Encode in the documented field order through the shared JSON writer.
    ///
    /// Decoded string values and original numeric spellings are preserved. The writer
    /// handles escaping, work, depth and fallible output storage; no source DOM
    /// is copied. Equivalent differently spelled numbers remain distinguishable.
    ///
    /// # Errors
    /// Returns the original resource/cancellation refusal located at this profile.
    /// No partial JSON is returned. See [`Self`] for an executable example.
    pub fn encode_json(&self, governor: &mut Governor) -> Result<String, EnvelopeError> {
        let fields: [JsonField<'_>; 10] = core::array::from_fn(|index| JsonField {
            key: FIELDS[index],
            value: Some(self.fields[index]),
        });
        let encoded = json::lines_to_json_scratch(&[JsonLine::Object(&fields)], governor)
            .map_err(|cause| error(self.original, EnvelopeProblem::Resource(cause)))?;
        let mut output = encoded.into_string();
        // The shared line writer guarantees exactly one framing LF per root.
        // Remove only that framing byte; embedded newlines remain JSON escapes.
        let terminator = output.pop();
        debug_assert_eq!(terminator, Some('\n'));
        Ok(output)
    }
}

fn quantity(
    source: &Node,
    field: &'static str,
    governor: &mut Governor,
) -> Result<u64, EnvelopeError> {
    step(source, Resource::Work, 1, governor)?;
    let Value::Number(number) = &source.value else {
        return Err(error(source, EnvelopeProblem::WrongType(field)));
    };
    let reading = number
        .integer_magnitude_governed(governor)
        .map_err(|cause| error(source, EnvelopeProblem::Resource(cause)))?;
    let problem = match reading {
        IntegerReading::Exact { negative: true, .. }
        | IntegerReading::Exact { magnitude: 0, .. } => ExecutionQuantityProblem::NonPositive,
        IntegerReading::Exact { magnitude, .. } => {
            if let Ok(value) = u64::try_from(magnitude) {
                return Ok(value);
            }
            ExecutionQuantityProblem::OutOfRange
        }
        IntegerReading::Fractional => ExecutionQuantityProblem::Fractional,
        IntegerReading::OutOfRange => ExecutionQuantityProblem::OutOfRange,
    };
    Err(error(
        source,
        EnvelopeProblem::ExecutionQuantity { field, problem },
    ))
}
