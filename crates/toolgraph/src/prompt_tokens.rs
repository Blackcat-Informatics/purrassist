// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Separate token admission over the existing complete prompt representation.

use std::fmt;

use purrassist_core::{Governor, StopReason, contract::Resource};

use crate::{FramedPrompt, FramedPromptBytes, PromptByteFailure};

/// Deterministic, caller-supplied counting for exactly the bytes supplied.
///
/// Implementations must be pure, admit work/storage through the supplied
/// governor, and count all tokens of their named tokenizer configuration,
/// including any implicit tokens. The caller establishes applicability to its
/// model and representation. Metadata must be prepared by the caller before
/// admission; the identity methods only borrow that metadata. This library
/// neither qualifies tokenizers nor verifies arbitrary callback code. Identity
/// labels record that caller contract.
pub trait TokenCounter: fmt::Debug {
    /// Nonempty tokenizer/artifact identity, retained verbatim.
    fn identity(&self) -> &str;
    /// Nonempty configuration identity, including preprocessing and special tokens.
    fn configuration(&self) -> &str;
    /// Count the complete supplied representation exactly, without changing it.
    ///
    /// # Errors
    /// Return [`TokenCountError::Unavailable`] when this counter cannot count the
    /// representation, or preserve its native governor/allocation refusal.
    fn count(&self, bytes: &[u8], governor: &mut Governor) -> Result<u64, TokenCountError>;
}

/// A counter refusal; absence of a count is never a zero count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenCountError {
    /// The named counter cannot supply a count for this representation.
    Unavailable,
    /// Original native resource or cancellation failure.
    Resource(StopReason),
}

impl From<StopReason> for TokenCountError {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}

/// Caller assertions permitting bytes to bound tokens conservatively.
///
/// Supplying this contract asserts **all** of the following for the exact
/// assembled bytes: at most one token per input byte, no uncounted implicit
/// special tokens, and no representation-expanding preprocessing. These are
/// recorded caller assumptions, not library verification or qualification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteLevelTokenContract<'a> {
    /// Nonempty identity of the encoding and its configuration, unchanged.
    pub encoding_identity: &'a str,
    /// Nonempty caller-owned basis for admitting all three assumptions.
    pub admission_basis: &'a str,
}

/// Explicit counting choice; no variant silently falls back to another.
#[derive(Clone, Copy, Debug)]
pub enum PromptTokenAccounting<'a> {
    /// No applicable counter or caller-admitted encoding is available.
    Unavailable,
    /// Exact count supplied by this deterministic caller counter.
    Exact(&'a dyn TokenCounter),
    /// Conservative bound under the caller's explicit encoding assumptions.
    ByteLevel(ByteLevelTokenContract<'a>),
}

/// Recorded method and original identities of a token measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptTokenBasis<'a> {
    /// Exact count under the named caller counter and configuration.
    Exact {
        /// Original nonempty tokenizer identity.
        identity: &'a str,
        /// Original nonempty configuration identity.
        configuration: &'a str,
    },
    /// Byte-derived upper bound, with all three caller assertions retained.
    ByteLevel(ByteLevelTokenContract<'a>),
}

/// The distinct byte and token quantities for one complete representation.
///
/// [`AccountedPrompt::bytes`] is the exact immutable representation counted.
/// On refusal the original [`PromptTokenError::input`] supplies every byte in
/// its original order. A byte-derived `tokens` value is an upper bound, not an
/// exact count; [`Self::basis`] makes that distinction explicit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PromptTokenMeasurement<'a> {
    /// Exact complete UTF-8 bytes, independent of tokenization.
    pub exact_bytes: u64,
    /// Exact token count or conservative upper bound according to `basis`.
    pub tokens: u64,
    /// Original identity/configuration or caller-admitted assumptions.
    pub basis: PromptTokenBasis<'a>,
}

/// Complete immutable representation admitted under both independent ceilings.
#[derive(Debug, PartialEq, Eq)]
pub struct AccountedPrompt<'input, 'accounting> {
    bytes: FramedPromptBytes<'input>,
    measurement: PromptTokenMeasurement<'accounting>,
}

impl<'input, 'accounting> AccountedPrompt<'input, 'accounting> {
    /// Borrow the exact counted bytes and original input without mutable access.
    #[must_use]
    pub const fn bytes(&self) -> &FramedPromptBytes<'input> {
        &self.bytes
    }

    /// Read the separate exact-byte and token measurement.
    #[must_use]
    pub const fn measurement(&self) -> PromptTokenMeasurement<'accounting> {
        self.measurement
    }
}

/// Which original accounting identity was empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptTokenIdentity {
    /// Exact counter artifact identity.
    Counter,
    /// Exact counter configuration identity.
    Configuration,
    /// Conservative encoding/configuration identity.
    Encoding,
    /// Caller basis for the conservative encoding assumptions.
    AdmissionBasis,
}

/// Why complete token admission failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptTokenFailure {
    /// Independent exact-byte sizing or ceiling failure.
    Bytes(PromptByteFailure),
    /// Neither explicit accounting contract was supplied.
    AccountingUnavailable,
    /// The supplied exact counter could not count this representation.
    CounterUnavailable,
    /// An identity was empty; original text is never trimmed or normalized.
    InvalidIdentity(PromptTokenIdentity),
    /// The recorded measurement exceeds this independent token ceiling.
    TokenCeiling {
        /// Caller-supplied maximum exact count or conservative upper bound.
        max_tokens: u64,
    },
    /// Native allocation, exhaustion or cancellation refusal, unchanged.
    Resource(StopReason),
}

/// Atomic refusal retaining original input, accounting basis and any measurement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptTokenError<'input, 'accounting> {
    /// Every original declaration and framing occurrence, unchanged.
    pub input: FramedPrompt<'input>,
    /// Original accounting labels once captured; a pre-existing native stop
    /// refuses before invoking metadata methods.
    pub basis: Option<PromptTokenBasis<'accounting>>,
    /// Complete measurement if counting completed before refusal.
    pub measurement: Option<PromptTokenMeasurement<'accounting>>,
    /// Typed reason; no successful or truncated output accompanies a refusal.
    pub reason: PromptTokenFailure,
}

impl fmt::Display for PromptTokenError<'_, '_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "prompt token admission failed: {:?}",
            self.reason
        )
    }
}
impl std::error::Error for PromptTokenError<'_, '_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.reason {
            PromptTokenFailure::Resource(reason)
            | PromptTokenFailure::Bytes(PromptByteFailure::Resource(reason)) => Some(reason),
            _ => None,
        }
    }
}

impl<'input> FramedPrompt<'input> {
    /// Admit complete prompt bytes and a separately measured token quantity.
    ///
    /// The byte ceiling is checked before the exact counter is called. Counting
    /// sees every framing byte once; byte-derived accounting uses that same
    /// complete representation. Counter identity/configuration are captured once
    /// before invocation. Output storage stays release-owned through counting,
    /// token-ceiling admission and the final governor check.
    ///
    /// # Errors
    /// Returns typed unavailable/invalid-identity, byte/token ceiling, or native
    /// resource refusal. Every failure retains the original input and any
    /// accounting labels already captured; a completed count remains visible on
    /// later refusal. No callback failure selects byte-derived accounting implicitly.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor};
    /// use purrassist_toolgraph::{FramedPrompt, PromptTokenAccounting, ByteLevelTokenContract};
    /// let input = FramedPrompt { source_identity: "empty", prefix: "[", separator: ",", suffix: "]", parts: &[] };
    /// let accounting = PromptTokenAccounting::ByteLevel(ByteLevelTokenContract {
    ///     encoding_identity: "one-byte-one-token.v1",
    ///     admission_basis: "caller admits the three byte-level assumptions",
    /// });
    /// let output = input.assemble_accounted(2, 2, accounting, &mut Governor::new(Budget::default()))?;
    /// assert_eq!(output.bytes().text, "[]");
    /// assert_eq!(output.measurement().tokens, 2);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[expect(
        clippy::result_large_err,
        reason = "retain complete borrowed input, accounting labels and measured refusal without allocating to report exhaustion"
    )]
    pub fn assemble_accounted<'accounting>(
        self,
        max_bytes: u64,
        max_tokens: u64,
        accounting: PromptTokenAccounting<'accounting>,
        governor: &mut Governor,
    ) -> Result<AccountedPrompt<'input, 'accounting>, PromptTokenError<'input, 'accounting>> {
        governor.check().map_err(|reason| PromptTokenError {
            input: self,
            basis: None,
            measurement: None,
            reason: PromptTokenFailure::Resource(reason),
        })?;
        let basis = accounting.basis();
        let failure = |reason, measurement| PromptTokenError {
            input: self,
            basis,
            measurement,
            reason,
        };
        let basis =
            basis.ok_or_else(|| failure(PromptTokenFailure::AccountingUnavailable, None))?;
        validate_basis(basis, governor).map_err(|reason| failure(reason, None))?;
        let (text, exact_bytes) = self.assemble(max_bytes, governor).map_err(|reason| {
            failure(
                match reason {
                    PromptByteFailure::Resource(reason) => PromptTokenFailure::Resource(reason),
                    other => PromptTokenFailure::Bytes(other),
                },
                None,
            )
        })?;
        let counted = match accounting {
            PromptTokenAccounting::Exact(counter) => counter.count(text.as_bytes(), governor),
            PromptTokenAccounting::ByteLevel(_) => Ok(exact_bytes),
            PromptTokenAccounting::Unavailable => unreachable!("a basis was admitted"),
        };
        let measurement = counted.as_ref().ok().map(|&tokens| PromptTokenMeasurement {
            exact_bytes,
            tokens,
            basis,
        });
        // A callback may return success or unavailable after stopping its governor.
        // The native stop remains authoritative in either case.
        governor
            .check()
            .map_err(|reason| failure(PromptTokenFailure::Resource(reason), measurement))?;
        let tokens = counted.map_err(|reason| {
            failure(
                match reason {
                    TokenCountError::Unavailable => PromptTokenFailure::CounterUnavailable,
                    TokenCountError::Resource(reason) => PromptTokenFailure::Resource(reason),
                },
                None,
            )
        })?;
        let measurement = PromptTokenMeasurement {
            exact_bytes,
            tokens,
            basis,
        };
        if tokens > max_tokens {
            return Err(failure(
                PromptTokenFailure::TokenCeiling { max_tokens },
                Some(measurement),
            ));
        }
        governor
            .check()
            .map_err(|reason| failure(PromptTokenFailure::Resource(reason), Some(measurement)))?;
        Ok(AccountedPrompt {
            bytes: FramedPromptBytes {
                input: self,
                text: text.into_string(),
                exact_bytes,
            },
            measurement,
        })
    }
}

impl<'a> PromptTokenAccounting<'a> {
    fn basis(self) -> Option<PromptTokenBasis<'a>> {
        match self {
            Self::Unavailable => None,
            Self::Exact(counter) => Some(PromptTokenBasis::Exact {
                identity: counter.identity(),
                configuration: counter.configuration(),
            }),
            Self::ByteLevel(contract) => Some(PromptTokenBasis::ByteLevel(contract)),
        }
    }
}

fn validate_basis(
    basis: PromptTokenBasis<'_>,
    governor: &mut Governor,
) -> Result<(), PromptTokenFailure> {
    let identities = match basis {
        PromptTokenBasis::Exact {
            identity,
            configuration,
        } => [
            (identity, PromptTokenIdentity::Counter),
            (configuration, PromptTokenIdentity::Configuration),
        ],
        PromptTokenBasis::ByteLevel(contract) => [
            (contract.encoding_identity, PromptTokenIdentity::Encoding),
            (
                contract.admission_basis,
                PromptTokenIdentity::AdmissionBasis,
            ),
        ],
    };
    for (text, field) in identities {
        governor
            .charge(Resource::Work, 1)
            .map_err(PromptTokenFailure::Resource)?;
        let bytes = u64::try_from(text.len())
            .map_err(|_| PromptTokenFailure::Bytes(PromptByteFailure::LengthOverflow))?;
        governor
            .charge(Resource::InputBytes, bytes)
            .map_err(PromptTokenFailure::Resource)?;
        if text.is_empty() {
            return Err(PromptTokenFailure::InvalidIdentity(field));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ByteLevelTokenContract, PromptTokenAccounting, PromptTokenBasis, PromptTokenFailure,
        PromptTokenIdentity, PromptTokenMeasurement, TokenCountError, TokenCounter,
    };
    use crate::{FramedPrompt, PromptByteFailure, PromptBytePart};
    use core::cell::Cell;
    use purrassist_core::{Budget, CancellationToken, Governor, StopReason, contract::Resource};

    #[derive(Debug)]
    struct Counter {
        calls: Cell<u64>,
        identity: &'static str,
        configuration: &'static str,
        action: fn(&[u8], &mut Governor) -> Result<u64, TokenCountError>,
    }
    impl TokenCounter for Counter {
        fn identity(&self) -> &str {
            self.identity
        }
        fn configuration(&self) -> &str {
            self.configuration
        }
        fn count(&self, bytes: &[u8], governor: &mut Governor) -> Result<u64, TokenCountError> {
            self.calls.set(self.calls.get() + 1);
            (self.action)(bytes, governor)
        }
    }
    fn scalar_counter() -> Counter {
        Counter {
            calls: Cell::new(0),
            identity: "scalar",
            configuration: "identity/no-special-tokens",
            action: |bytes, governor| {
                governor.charge(Resource::Work, u64::try_from(bytes.len()).unwrap() * 2)?;
                let text = core::str::from_utf8(bytes).map_err(|_| TokenCountError::Unavailable)?;
                Ok(u64::try_from(text.chars().count()).unwrap())
            },
        }
    }
    fn prompt<'a>(parts: &'a [PromptBytePart<'a>]) -> FramedPrompt<'a> {
        FramedPrompt {
            source_identity: "source",
            prefix: "[",
            separator: ",",
            suffix: "]",
            parts,
        }
    }
    fn governor() -> Governor {
        Governor::new(Budget::default())
    }

    #[test]
    fn complete_representation_has_independent_byte_and_exact_or_conservative_token_limits() {
        let parts = [
            PromptBytePart {
                identity: "same",
                text: "Ω",
            },
            PromptBytePart {
                identity: "same",
                text: "😀",
            },
        ];
        let input = prompt(&parts);
        let counter = scalar_counter();
        let accounting = PromptTokenAccounting::Exact(&counter);
        let exact = input
            .assemble_accounted(9, 5, accounting, &mut governor())
            .unwrap();
        assert_eq!(exact.bytes().text, "[Ω,😀]");
        assert_eq!(exact.bytes().input, input);
        assert_eq!(
            (exact.measurement().exact_bytes, exact.measurement().tokens),
            (9, 5)
        );
        assert_eq!(counter.calls.get(), 1);
        let mut g = governor();
        let refusal = input
            .assemble_accounted(9, 4, accounting, &mut g)
            .unwrap_err();
        assert_eq!(refusal.input, input);
        assert_eq!(refusal.measurement, Some(exact.measurement()));
        assert_eq!(
            refusal.reason,
            PromptTokenFailure::TokenCeiling { max_tokens: 4 }
        );
        assert_eq!(g.scratch_meter().live(), 0);
        assert_eq!(g.check(), Ok(()));
        let calls = counter.calls.get();
        let refusal = input
            .assemble_accounted(5, 5, accounting, &mut governor())
            .unwrap_err();
        assert_eq!(
            refusal.reason,
            PromptTokenFailure::Bytes(PromptByteFailure::ByteCeiling {
                required_bytes: 9,
                max_bytes: 5
            })
        );
        assert_eq!(counter.calls.get(), calls);
        assert_eq!(refusal.measurement, None);
        let contract = ByteLevelTokenContract {
            encoding_identity: "scalar",
            admission_basis: "at least one byte per scalar; no implicit tokens; no preprocessing",
        };
        let accounting = PromptTokenAccounting::ByteLevel(contract);
        let refusal = input
            .assemble_accounted(9, 5, accounting, &mut governor())
            .unwrap_err();
        assert_eq!(
            refusal.measurement,
            Some(PromptTokenMeasurement {
                exact_bytes: 9,
                tokens: 9,
                basis: PromptTokenBasis::ByteLevel(contract)
            })
        );
        assert_eq!(
            refusal.reason,
            PromptTokenFailure::TokenCeiling { max_tokens: 5 }
        );
        assert_eq!(
            input
                .assemble_accounted(9, 9, accounting, &mut governor())
                .unwrap()
                .bytes()
                .text,
            exact.bytes().text
        );
        // Empty declaration lists still count both framing bytes.
        assert_eq!(
            prompt(&[])
                .assemble_accounted(2, 2, accounting, &mut governor())
                .unwrap()
                .measurement()
                .tokens,
            2
        );
    }

    #[test]
    fn missing_unavailable_and_invalid_contracts_refuse_without_fallback_or_normalization() {
        let input = prompt(&[]);
        let error = input
            .assemble_accounted(2, 2, PromptTokenAccounting::Unavailable, &mut governor())
            .unwrap_err();
        assert_eq!(error.input, input);
        assert_eq!(error.reason, PromptTokenFailure::AccountingUnavailable);
        let mut counter = scalar_counter();
        counter.action = |_, _| Err(TokenCountError::Unavailable);
        let mut g = governor();
        let error = input
            .assemble_accounted(2, 2, PromptTokenAccounting::Exact(&counter), &mut g)
            .unwrap_err();
        assert_eq!(error.reason, PromptTokenFailure::CounterUnavailable);
        assert_eq!(error.measurement, None);
        assert!(matches!(
            error.basis,
            Some(PromptTokenBasis::Exact {
                identity: "scalar",
                ..
            })
        ));
        assert_eq!(g.scratch_meter().live(), 0);
        for field in [
            PromptTokenIdentity::Counter,
            PromptTokenIdentity::Configuration,
            PromptTokenIdentity::Encoding,
            PromptTokenIdentity::AdmissionBasis,
        ] {
            let mut counter = scalar_counter();
            let mut contract = ByteLevelTokenContract {
                encoding_identity: "encoding",
                admission_basis: "caller assertion",
            };
            let accounting = match field {
                PromptTokenIdentity::Counter => {
                    counter.identity = "";
                    PromptTokenAccounting::Exact(&counter)
                }
                PromptTokenIdentity::Configuration => {
                    counter.configuration = "";
                    PromptTokenAccounting::Exact(&counter)
                }
                PromptTokenIdentity::Encoding => {
                    contract.encoding_identity = "";
                    PromptTokenAccounting::ByteLevel(contract)
                }
                PromptTokenIdentity::AdmissionBasis => {
                    contract.admission_basis = "";
                    PromptTokenAccounting::ByteLevel(contract)
                }
            };
            let error = input
                .assemble_accounted(2, 2, accounting, &mut governor())
                .unwrap_err();
            assert_eq!(error.reason, PromptTokenFailure::InvalidIdentity(field));
            assert_eq!(counter.calls.get(), 0);
        }
        counter = scalar_counter();
        counter.identity = " original identity ";
        let output = input
            .assemble_accounted(
                2,
                2,
                PromptTokenAccounting::Exact(&counter),
                &mut governor(),
            )
            .unwrap();
        assert!(matches!(
            output.measurement().basis,
            PromptTokenBasis::Exact {
                identity: " original identity ",
                ..
            }
        ));
    }

    #[test]
    fn native_stops_win_and_unpublished_storage_releases_even_when_counter_ignores_stop() {
        let input = prompt(&[]);
        let mut counter = scalar_counter();
        counter.action = |_, governor| {
            assert!(governor.charge(Resource::Work, u64::MAX).is_err());
            Ok(2)
        };
        let mut g = governor();
        let error = input
            .assemble_accounted(2, 2, PromptTokenAccounting::Exact(&counter), &mut g)
            .unwrap_err();
        assert_eq!(
            error.reason,
            PromptTokenFailure::Resource(StopReason::BudgetExceeded(Resource::Work))
        );
        assert_eq!(error.measurement.unwrap().tokens, 2);
        assert_eq!(g.scratch_meter().live(), 0);
        let token = CancellationToken::new();
        let mut cancelled = Governor::with_cancellation(Budget::default(), token.clone());
        token.cancel();
        let calls = counter.calls.get();
        let error = input
            .assemble_accounted(2, 2, PromptTokenAccounting::Exact(&counter), &mut cancelled)
            .unwrap_err();
        assert_eq!(
            error.reason,
            PromptTokenFailure::Resource(StopReason::Cancelled)
        );
        assert_eq!(error.basis, None);
        assert_eq!(counter.calls.get(), calls);
        let mut g = Governor::new(Budget {
            scratch_bytes: 0,
            ..Budget::default()
        });
        let error = input
            .assemble_accounted(2, 2, PromptTokenAccounting::Exact(&counter), &mut g)
            .unwrap_err();
        assert_eq!(
            error.reason,
            PromptTokenFailure::Resource(StopReason::BudgetExceeded(Resource::ScratchBytes))
        );
        assert_eq!(counter.calls.get(), calls);
        assert_eq!(g.scratch_meter().live(), 0);
        counter.action = |_, _| Err(TokenCountError::Resource(StopReason::Cancelled));
        let mut g = governor();
        let error = input
            .assemble_accounted(2, 2, PromptTokenAccounting::Exact(&counter), &mut g)
            .unwrap_err();
        assert_eq!(
            error.reason,
            PromptTokenFailure::Resource(StopReason::Cancelled)
        );
        assert_eq!(g.scratch_meter().live(), 0);
    }
}
