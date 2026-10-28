// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Exact framed prompt bytes over caller-prepared declarations.

use std::fmt;

use purrassist_core::{Governor, ScratchString, StopReason, contract::Resource};

/// One original declaration and its opaque caller-owned identity.
///
/// Names need not be unique. Occurrences remain in the supplied order; neither
/// identity nor text is normalized or interpreted as an authorization claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PromptBytePart<'a> {
    /// Original declaration identity, retained verbatim on both outcomes.
    pub identity: &'a str,
    /// Already prepared UTF-8 declaration, copied without escaping or trimming.
    pub text: &'a str,
}

/// Complete explicit framing and ordered borrowed declarations.
///
/// This describes supplied bytes, not a verified model template or graph
/// projection. The caller prepares declarations and all framing separately.
/// Empty lists still emit `prefix` followed by `suffix`; separators occur only
/// between declarations. Source and part identities are opaque original labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FramedPrompt<'a> {
    /// Original source identity; no digest or authentication is inferred.
    pub source_identity: &'a str,
    /// Bytes before the first declaration, also emitted for an empty list.
    pub prefix: &'a str,
    /// Bytes between each adjacent pair of declarations.
    pub separator: &'a str,
    /// Bytes after the last declaration, also emitted for an empty list.
    pub suffix: &'a str,
    /// Original named occurrences in exact output order.
    pub parts: &'a [PromptBytePart<'a>],
}

/// Complete admitted prompt bytes and their original borrowed input.
///
/// This contains exact UTF-8 byte accounting only. Model acceptance, tokenizer
/// qualification, family placement, selection and graph ledgers are separate.
#[derive(Debug, PartialEq, Eq)]
pub struct FramedPromptBytes<'a> {
    /// Original identities, declarations and framing, unchanged.
    pub input: FramedPrompt<'a>,
    /// Complete concatenated UTF-8 output; never a partial declaration.
    pub text: String,
    /// Exact byte length of `text`, including every framing occurrence.
    pub exact_bytes: u64,
}

/// Why complete framed bytes could not be admitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptByteFailure {
    /// Complete framing and declarations exceed the independent byte ceiling.
    ByteCeiling {
        /// Exact complete requirement, without truncating or omitting a part.
        required_bytes: u64,
        /// Caller-supplied maximum number of prompt bytes.
        max_bytes: u64,
    },
    /// The complete byte requirement cannot fit the sizing or allocation index.
    LengthOverflow,
    /// Native governance or allocation refusal, preserving its original cause.
    Resource(StopReason),
}

/// An atomic refusal retaining the complete original borrowed input.
///
/// The byte-ceiling reason contains the exact requirement and allowed ceiling;
/// no successful artifact or partial output is returned on failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptByteError<'a> {
    /// Original source/part identities, text and framing, unchanged.
    pub input: FramedPrompt<'a>,
    /// Typed byte sizing, semantic ceiling or governor refusal.
    pub reason: PromptByteFailure,
}

impl fmt::Display for PromptByteError<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.reason {
            PromptByteFailure::ByteCeiling {
                required_bytes,
                max_bytes,
            } => write!(
                formatter,
                "framed prompt requires {required_bytes} bytes, exceeding the {max_bytes} byte ceiling"
            ),
            PromptByteFailure::LengthOverflow => {
                formatter.write_str("framed prompt byte length overflow")
            }
            PromptByteFailure::Resource(reason) => {
                write!(formatter, "framed prompt stopped: {reason}")
            }
        }
    }
}
impl std::error::Error for PromptByteError<'_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.reason {
            PromptByteFailure::Resource(reason) => Some(reason),
            _ => None,
        }
    }
}
impl From<StopReason> for PromptByteFailure {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}

impl<'a> FramedPrompt<'a> {
    /// Assemble and admit exactly the supplied complete representation.
    ///
    /// The semantic ceiling counts bytes independently of governor limits.
    /// Zero admits only a completely empty representation. Each original input
    /// admits its individual byte length; work precedes sizing/copying and
    /// output capacity is fallible governed storage. A final cancellation check
    /// precedes complete publication. No declaration is dropped or truncated.
    ///
    /// # Errors
    /// [`PromptByteFailure::ByteCeiling`] retains the exact required/allowed
    /// bytes; [`PromptByteFailure::LengthOverflow`] refuses unrepresentable
    /// sizing. Native governor and allocation causes are preserved in
    /// [`PromptByteFailure::Resource`]. Every error retains `self` unchanged.
    /// Resource exhaustion is distinct from a semantic byte-ceiling refusal.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor};
    /// use purrassist_toolgraph::{FramedPrompt, PromptBytePart, PromptByteFailure};
    /// let parts = [PromptBytePart { identity: "read", text: r#"{"name":"read"}"# }];
    /// let input = FramedPrompt { source_identity: "catalog", prefix: "[", separator: ",", suffix: "]", parts: &parts };
    /// let mut governor = Governor::new(Budget::default());
    /// let output = input.assemble_bytes(17, &mut governor).unwrap();
    /// assert_eq!(output.text, r#"[{"name":"read"}]"#);
    /// assert_eq!(output.exact_bytes, 17);
    /// let error = input.assemble_bytes(16, &mut governor).unwrap_err();
    /// assert_eq!(error.input, input);
    /// assert_eq!(error.reason, PromptByteFailure::ByteCeiling { required_bytes: 17, max_bytes: 16 });
    /// ```
    pub fn assemble_bytes(
        self,
        max_bytes: u64,
        governor: &mut Governor,
    ) -> Result<FramedPromptBytes<'a>, PromptByteError<'a>> {
        self.assemble(max_bytes, governor)
            .map(|(text, exact_bytes)| FramedPromptBytes {
                input: self,
                text: text.into_string(),
                exact_bytes,
            })
            .map_err(|reason| PromptByteError {
                input: self,
                reason,
            })
    }

    pub(crate) fn assemble(
        self,
        max_bytes: u64,
        governor: &mut Governor,
    ) -> Result<(ScratchString, u64), PromptByteFailure> {
        governor.check()?;
        for text in [
            self.source_identity,
            self.prefix,
            self.separator,
            self.suffix,
        ] {
            admit_text(text, governor)?;
        }
        let mut declarations_bytes = 0_u64;
        for part in self.parts {
            admit_text(part.identity, governor)?;
            admit_text(part.text, governor)?;
            governor.charge(Resource::Work, 1)?;
            declarations_bytes = declarations_bytes
                .checked_add(byte_length(part.text)?)
                .ok_or(PromptByteFailure::LengthOverflow)?;
        }
        // Admission precedes the fixed number of length reads and checked
        // operations in the complete framing formula.
        governor.charge(Resource::Work, 12)?;
        let required_bytes = framed_length(
            byte_length(self.prefix)?,
            byte_length(self.separator)?,
            byte_length(self.suffix)?,
            declarations_bytes,
            u64::try_from(self.parts.len()).map_err(|_| PromptByteFailure::LengthOverflow)?,
        )?;
        if required_bytes > max_bytes {
            return Err(PromptByteFailure::ByteCeiling {
                required_bytes,
                max_bytes,
            });
        }
        let capacity =
            usize::try_from(required_bytes).map_err(|_| PromptByteFailure::LengthOverflow)?;
        // Every byte copied below is admitted before allocating or copying it.
        governor.charge(Resource::Work, required_bytes)?;
        let mut output = ScratchString::new(governor);
        output.reserve(capacity)?;
        output.push_str(self.prefix)?;
        for (index, part) in self.parts.iter().enumerate() {
            governor.charge(Resource::Work, 1)?;
            if index > 0 {
                output.push_str(self.separator)?;
            }
            output.push_str(part.text)?;
        }
        output.push_str(self.suffix)?;
        governor.check()?;
        Ok((output, required_bytes))
    }
}

fn byte_length(text: &str) -> Result<u64, PromptByteFailure> {
    u64::try_from(text.len()).map_err(|_| PromptByteFailure::LengthOverflow)
}
fn admit_text(text: &str, governor: &mut Governor) -> Result<(), PromptByteFailure> {
    governor.charge(Resource::Work, 1)?;
    governor.charge(Resource::InputBytes, byte_length(text)?)?;
    Ok(())
}

/// Production formula, also exercised with lengths too large to allocate.
fn framed_length(
    prefix: u64,
    separator: u64,
    suffix: u64,
    declarations: u64,
    part_count: u64,
) -> Result<u64, PromptByteFailure> {
    let separators = if part_count == 0 { 0 } else { part_count - 1 };
    prefix
        .checked_add(suffix)
        .and_then(|length| length.checked_add(declarations))
        .and_then(|length| {
            separator
                .checked_mul(separators)
                .and_then(|between| length.checked_add(between))
        })
        .ok_or(PromptByteFailure::LengthOverflow)
}

#[cfg(test)]
mod tests {
    use super::{FramedPrompt, PromptByteFailure, PromptBytePart, framed_length};
    use purrassist_core::{Budget, CancellationToken, Governor, StopReason, contract::Resource};

    fn governor() -> Governor {
        Governor::new(Budget::default())
    }
    fn input<'a>(parts: &'a [PromptBytePart<'a>]) -> FramedPrompt<'a> {
        FramedPrompt {
            source_identity: "original-catalog",
            prefix: "[\n",
            separator: ",\n",
            suffix: "\n]",
            parts,
        }
    }

    #[test]
    fn complete_framing_preserves_empty_unicode_escaped_and_named_occurrences() {
        let empty = input(&[]);
        let output = empty.assemble_bytes(4, &mut governor()).unwrap();
        assert_eq!(output.text, "[\n\n]");
        assert_eq!(output.exact_bytes, 4);
        assert_eq!(output.input, empty);

        let parts = [
            PromptBytePart {
                identity: "same-original-name",
                text: "Ω😀",
            },
            PromptBytePart {
                identity: "same-original-name",
                text: r#"{"escaped":"\"\\\n"}"#,
            },
            PromptBytePart {
                identity: "",
                text: "  unchanged  ",
            },
        ];
        let original = input(&parts);
        let expected = format!(
            "[\n{},\n{},\n{}\n]",
            parts[0].text, parts[1].text, parts[2].text
        );
        let output = original.assemble_bytes(u64::MAX, &mut governor()).unwrap();
        assert_eq!(output.text, expected);
        assert_eq!(output.exact_bytes, u64::try_from(expected.len()).unwrap());
        assert_eq!(output.input, original);
        let single = FramedPrompt {
            parts: &parts[..1],
            ..original
        };
        assert_eq!(
            single.assemble_bytes(10, &mut governor()).unwrap().text,
            "[\nΩ😀\n]"
        );
    }

    #[test]
    fn byte_ceiling_and_checked_complete_framing_refuse_without_a_partial_artifact() {
        let empty = FramedPrompt {
            prefix: "",
            separator: "unused",
            suffix: "",
            ..input(&[])
        };
        assert_eq!(empty.assemble_bytes(0, &mut governor()).unwrap().text, "");
        let framed = input(&[]);
        let mut g = governor();
        let error = framed.assemble_bytes(0, &mut g).unwrap_err();
        assert_eq!(error.input, framed);
        assert_eq!(
            error.reason,
            PromptByteFailure::ByteCeiling {
                required_bytes: 4,
                max_bytes: 0
            }
        );
        assert_eq!(g.scratch_meter().live(), 0);
        // A semantic byte refusal does not exhaust an otherwise usable governor.
        assert_eq!(g.check(), Ok(()));

        let parts = [
            PromptBytePart {
                identity: "first",
                text: "Ω",
            },
            PromptBytePart {
                identity: "second",
                text: "x",
            },
        ];
        let original = input(&parts);
        let exact = original.assemble_bytes(9, &mut g).unwrap();
        assert_eq!(exact.text, "[\nΩ,\nx\n]");
        let error = original.assemble_bytes(8, &mut g).unwrap_err();
        assert_eq!(error.input, original);
        assert_eq!(
            error.reason,
            PromptByteFailure::ByteCeiling {
                required_bytes: 9,
                max_bytes: 8
            }
        );

        // Exercise the actual production formula at unreachable allocation
        // lengths: zero/one parts ignore even a huge unused separator.
        assert_eq!(framed_length(2, u64::MAX, 3, 0, 0), Ok(5));
        assert_eq!(framed_length(2, u64::MAX, 3, 7, 1), Ok(12));
        assert_eq!(framed_length(40, 3, 7, u64::MAX - 50, 2), Ok(u64::MAX));
        assert_eq!(
            framed_length(40, 3, 8, u64::MAX - 50, 2),
            Err(PromptByteFailure::LengthOverflow)
        );
        assert_eq!(
            framed_length(0, 2, 0, 0, u64::MAX),
            Err(PromptByteFailure::LengthOverflow)
        );
        assert_eq!(
            framed_length(u64::MAX, 0, 1, 0, 0),
            Err(PromptByteFailure::LengthOverflow)
        );
    }

    #[test]
    fn governor_boundaries_are_independent_sticky_and_release_unpublished_output() {
        let parts = [PromptBytePart {
            identity: "declaration",
            text: "payload",
        }];
        let original = input(&parts);
        let mut measured = governor();
        let output = original.assemble_bytes(11, &mut measured).unwrap();
        assert_eq!(output.text, "[\npayload\n]");
        let usage = measured.usage();
        let exact = Budget {
            work: usage.work,
            scratch_bytes: usage.peak_scratch_bytes,
            input_bytes: u64::try_from(original.source_identity.len()).unwrap(),
            ..Budget::default()
        };
        let mut g = Governor::new(exact);
        assert_eq!(
            original.assemble_bytes(11, &mut g).unwrap().text,
            output.text
        );
        for (budget, resource) in [
            (
                Budget {
                    work: exact.work - 1,
                    ..exact
                },
                Resource::Work,
            ),
            (
                Budget {
                    scratch_bytes: exact.scratch_bytes - 1,
                    ..exact
                },
                Resource::ScratchBytes,
            ),
            (
                Budget {
                    input_bytes: exact.input_bytes - 1,
                    ..exact
                },
                Resource::InputBytes,
            ),
        ] {
            let mut g = Governor::new(budget);
            let error = original.assemble_bytes(u64::MAX, &mut g).unwrap_err();
            assert_eq!(error.input, original);
            let cause = StopReason::BudgetExceeded(resource);
            assert_eq!(error.reason, PromptByteFailure::Resource(cause.clone()));
            assert_eq!(g.check(), Err(cause));
            assert_eq!(g.scratch_meter().live(), 0);
        }
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let mut g = Governor::with_cancellation(exact, cancellation);
        let error = original.assemble_bytes(u64::MAX, &mut g).unwrap_err();
        assert_eq!(error.input, original);
        assert_eq!(
            error.reason,
            PromptByteFailure::Resource(StopReason::Cancelled)
        );
        assert_eq!(g.usage().work, 0);
        assert_eq!(g.scratch_meter().live(), 0);
    }
}
