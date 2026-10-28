// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Exact output byte bounds, separate from prompt token estimates.

mod codec;
mod field_mask;
pub use field_mask::{FieldMask, FieldMaskError, FieldMaskKind};
mod project;
pub use codec::{ContextCodecError, ContextManagement, LoadingStrategy, MCP_CONTEXT_FORMAT};
pub use project::{
    NullSelection, OutputTransform, ProjectedResult, ProjectionFailure, ProjectionStage,
    ProjectionTrace, ResultProjectionError,
};

use std::{fmt, num::NonZeroU64};

use crate::{Governor, StopReason, contract::Resource};

/// Unit measured in an output-bound receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputUnit {
    /// Bytes of the complete serialized result supplied for release.
    Bytes,
}

/// Enforcement applied to an output bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputEnforcement {
    /// Compare the exact byte length, without estimating tokens or truncating.
    ExactBytes,
}

/// Exact measurement retained on both successful and refused release checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputMeasurement {
    /// Declared positive bound in `unit`.
    pub limit: u64,
    /// Measured size of the complete supplied representation in `unit`.
    pub measured: u64,
    /// The declared and measured unit.
    pub unit: OutputUnit,
    /// The actual enforcement used.
    pub enforcement: OutputEnforcement,
}

/// Why an output byte contract cannot be admitted or satisfied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputLimitError {
    /// Zero is not a valid declared byte ceiling.
    ZeroLimit,
    /// The complete result exceeds the bound; the original bytes are untouched.
    Exceeded(OutputMeasurement),
    /// Caller governance refused the check, including cancellation.
    Resource(StopReason),
}

impl fmt::Display for OutputLimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroLimit => f.write_str("output byte limit must be positive"),
            Self::Exceeded(measurement) => write!(
                f,
                "output has {} bytes, exceeding the exact {} byte limit",
                measurement.measured, measurement.limit
            ),
            Self::Resource(reason) => write!(f, "output byte check stopped: {reason:?}"),
        }
    }
}

impl std::error::Error for OutputLimitError {}

/// A positive exact byte bound for a serialized tool result.
///
/// This contract makes no token-count claim. Apply it to the complete final
/// representation after field projection and serialization, before releasing
/// any bytes. An oversize result is refused intact rather than truncated.
/// Graph loading, manifest decoding and host release wiring are separate callers.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_core::output::{OutputByteLimit, OutputLimitError};
/// let bound = OutputByteLimit::new(4).unwrap();
/// let mut governor = Governor::new(Budget::default());
/// assert_eq!(bound.check(b"null", &mut governor).unwrap().measured, 4);
/// assert!(matches!(bound.check(b"false", &mut governor), Err(OutputLimitError::Exceeded(_))));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputByteLimit(NonZeroU64);

impl OutputByteLimit {
    /// Admit a positive byte ceiling, without interpreting it as a token count.
    ///
    /// # Errors
    /// Returns [`OutputLimitError::ZeroLimit`] for zero. See the type example
    /// for construction followed by exact output enforcement.
    pub const fn new(bytes: u64) -> Result<Self, OutputLimitError> {
        match NonZeroU64::new(bytes) {
            Some(bytes) => Ok(Self(bytes)),
            None => Err(OutputLimitError::ZeroLimit),
        }
    }

    /// The declared exact byte ceiling; see the type example for enforcement.
    #[must_use]
    pub const fn max_bytes(self) -> u64 {
        self.0.get()
    }

    /// Measure the complete serialized output without allocating or modifying it.
    ///
    /// The check admits the supplied input bytes and constant comparison work.
    /// It does not parse or serialize, scan content, count tokens, or invent a
    /// fallback when governance refuses the check. See the type example.
    ///
    /// # Errors
    /// Returns [`OutputLimitError::Exceeded`] with exact units and measurement
    /// for an oversize result, or [`OutputLimitError::Resource`] when admission
    /// or cancellation prevents checking it. No successful receipt is returned
    /// after a governor failure.
    pub fn check(
        self,
        serialized_output: &[u8],
        governor: &mut Governor,
    ) -> Result<OutputMeasurement, OutputLimitError> {
        let measured = crate::contract::saturating_size(serialized_output.len());
        governor
            .charge(Resource::InputBytes, measured)
            .map_err(OutputLimitError::Resource)?;
        governor
            .charge(Resource::Work, 1)
            .map_err(OutputLimitError::Resource)?;
        let measurement = OutputMeasurement {
            limit: self.max_bytes(),
            measured,
            unit: OutputUnit::Bytes,
            enforcement: OutputEnforcement::ExactBytes,
        };
        if measured > measurement.limit {
            Err(OutputLimitError::Exceeded(measurement))
        } else {
            Ok(measurement)
        }
    }
}
