// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Exact finite profile limits for the selected offline `AppArmor` representation.

use core::fmt;
use purrassist_core::{
    Governor, StopReason,
    contract::{Resource, format_output_governed, saturating_size as size},
};

const MAX_DECIMAL: u64 = 9_223_372_036_854_775_807;
const MAX_CPU_SECONDS: u64 = 9_223_372_036_854;

/// Original, caller-prepared profile limits; every field is mandatory.
///
/// Values are positive and finite when rendered. The selected `AppArmor` 4.1.7
/// representation admits bare decimal bytes/counts through `i64::MAX`, larger
/// byte values only when divisible by 1024, and CPU through 9223372036854 whole
/// seconds. Unsupported values are refused rather than rounded or clamped.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::ResourceLimits;
/// let limits = ResourceLimits { address_space_bytes: 1025, file_size_bytes: 1,
///     processes: 2, cpu_seconds: 3, open_files: 4 };
/// let fragment = limits.render("caller:limits", &mut Governor::new(Budget::default())).unwrap();
/// assert_eq!(fragment.limits, limits);
/// assert_eq!(fragment.text, "set rlimit as <= 1025,\nset rlimit fsize <= 1,\nset rlimit nproc <= 2,\nset rlimit cpu <= 3 seconds,\nset rlimit nofile <= 4,\n");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceLimits {
    /// Exact address-space bytes, never an infinity sentinel.
    pub address_space_bytes: u64,
    /// Exact maximum file size in bytes.
    pub file_size_bytes: u64,
    /// Exact positive process count.
    pub processes: u64,
    /// Exact positive whole seconds of CPU time.
    pub cpu_seconds: u64,
    /// Exact positive open-file count.
    pub open_files: u64,
}

/// The original field refused by the selected finite representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceLimitField {
    /// Address-space bytes.
    AddressSpaceBytes,
    /// File-size bytes.
    FileSizeBytes,
    /// Process count.
    Processes,
    /// Whole CPU seconds.
    CpuSeconds,
    /// Open-file count.
    OpenFiles,
}

/// A value refusal, distinct from an exhausted governor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceLimitProblem {
    /// All five fields require positive values, even where native zero exists.
    Zero,
    /// No exact finite encoding in this selected parser representation.
    UnsupportedRepresentation,
}

/// Complete original input plus its precise semantic or resource refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceLimitError<'a> {
    /// Invalid field; no output has been formatted.
    Value {
        /// Original caller identity.
        source: &'a str,
        /// All five unchanged original values.
        limits: ResourceLimits,
        /// First invalid field in declaration order.
        field: ResourceLimitField,
        /// Positive-value or selected-representation refusal.
        problem: ResourceLimitProblem,
    },
    /// Exact governance cause, without any published partial fragment.
    Resource {
        /// Original caller identity.
        source: &'a str,
        /// All five unchanged original values.
        limits: ResourceLimits,
        /// Original input/work/storage/cancellation failure.
        reason: StopReason,
    },
}
impl fmt::Display for ResourceLimitError<'_> {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "profile resource limits refused: {self:?}")
    }
}
impl core::error::Error for ResourceLimitError<'_> {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Resource { reason, .. } => Some(reason),
            Self::Value { .. } => None,
        }
    }
}

/// Selected offline target; it does not attest loaded policy or enforcement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceLimitRequirement {
    /// Explicit native policy ABI.
    pub abi: &'static str,
    /// Required native mediation family.
    pub mediation: &'static str,
}

/// All five complete declarations, with unchanged original numeric provenance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceLimitFragment<'a> {
    /// Original caller identity, retained as data and never interpolated in policy.
    pub source: &'a str,
    /// Every unchanged caller value, including bytes encoded with a K suffix.
    pub limits: ResourceLimits,
    /// Complete declarations in as, fsize, nproc, cpu, nofile order.
    pub text: String,
    /// Explicit offline target requirement.
    pub requirement: ResourceLimitRequirement,
}

impl ResourceLimits {
    /// Render all five limits atomically using the existing governed formatter.
    ///
    /// # Errors
    /// Zero or unsupported selected representations retain the field and complete
    /// original input. Resource failures preserve the exact governor cause.
    /// The source input ceiling, bounded numeric computation, actual writes and
    /// storage growth are admitted; cancellation is checked before publication.
    /// No partial text escapes. Preparing the caller's numbers is a separate operation.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor};
    /// use purrassist_toolgraph::{ResourceLimits, ResourceLimitError, ResourceLimitField,
    ///     ResourceLimitProblem};
    /// let limits = ResourceLimits { address_space_bytes: u64::MAX, file_size_bytes: 1,
    ///     processes: 1, cpu_seconds: 1, open_files: 1 };
    /// assert!(matches!(limits.render("caller:limits", &mut Governor::new(Budget::default())),
    ///     Err(ResourceLimitError::Value { field: ResourceLimitField::AddressSpaceBytes,
    ///         problem: ResourceLimitProblem::UnsupportedRepresentation, .. })));
    /// ```
    pub fn render<'a>(
        self,
        source: &'a str,
        governor: &mut Governor,
    ) -> Result<ResourceLimitFragment<'a>, ResourceLimitError<'a>> {
        let resource = |reason| ResourceLimitError::Resource {
            source,
            limits: self,
            reason,
        };
        governor
            .charge(Resource::InputBytes, size(source.len()))
            .map_err(resource)?;
        // Five bounded validations, two optional exact divisions and five u64
        // decimal displays (at most twenty digits each) fit this fixed reservation.
        // The shared sink separately admits every output byte and actual growth.
        governor.charge(Resource::Work, 512).map_err(resource)?;
        for (field, value, supported) in [
            (
                ResourceLimitField::AddressSpaceBytes,
                self.address_space_bytes,
                self.address_space_bytes <= MAX_DECIMAL
                    || self.address_space_bytes.is_multiple_of(1024),
            ),
            (
                ResourceLimitField::FileSizeBytes,
                self.file_size_bytes,
                self.file_size_bytes <= MAX_DECIMAL || self.file_size_bytes.is_multiple_of(1024),
            ),
            (
                ResourceLimitField::Processes,
                self.processes,
                self.processes <= MAX_DECIMAL,
            ),
            (
                ResourceLimitField::CpuSeconds,
                self.cpu_seconds,
                self.cpu_seconds <= MAX_CPU_SECONDS,
            ),
            (
                ResourceLimitField::OpenFiles,
                self.open_files,
                self.open_files <= MAX_DECIMAL,
            ),
        ] {
            let problem = if value == 0 {
                Some(ResourceLimitProblem::Zero)
            } else if !supported {
                Some(ResourceLimitProblem::UnsupportedRepresentation)
            } else {
                None
            };
            if let Some(problem) = problem {
                return Err(ResourceLimitError::Value {
                    source,
                    limits: self,
                    field,
                    problem,
                });
            }
        }
        let (address, address_unit) = byte_encoding(self.address_space_bytes);
        let (file, file_unit) = byte_encoding(self.file_size_bytes);
        let text = format_output_governed(
            format_args!(
                "set rlimit as <= {address}{address_unit},\nset rlimit fsize <= {file}{file_unit},\nset rlimit nproc <= {},\nset rlimit cpu <= {} seconds,\nset rlimit nofile <= {},\n",
                self.processes, self.cpu_seconds, self.open_files,
            ), governor,
        ).map_err(resource)?;
        governor.check().map_err(resource)?;
        Ok(ResourceLimitFragment {
            source,
            limits: self,
            text,
            requirement: ResourceLimitRequirement {
                abi: "3.0",
                mediation: "rlimit",
            },
        })
    }
}

const fn byte_encoding(value: u64) -> (u64, &'static str) {
    if value <= MAX_DECIMAL {
        (value, "")
    } else {
        (value / 1024, "K")
    }
}
