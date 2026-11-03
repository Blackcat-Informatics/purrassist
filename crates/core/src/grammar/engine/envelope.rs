// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Source evidence retained before an attempt's capture window retires.

use super::{Part, Parts};
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, ScratchCharge, ScratchVec, StopReason, push_scratch};
use crate::grammar::CaptureKind;
use crate::grammar::edit::{EditOperator, MarkupEdit};
use crate::tool_call::{DialectId, RawCall};
use core::ops::Range;

/// The exact extraction-retained range and its source-proven whitespace trim.
/// This is framing evidence only; it never establishes invocation equality.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SourceEnvelope {
    retained_start: usize,
    retained_end: usize,
    trimmed_start: usize,
    trimmed_end: usize,
}

impl ScratchCharge for SourceEnvelope {
    const CHARGE: usize = 32;
}

impl SourceEnvelope {
    pub(crate) fn matches(&self, retained: &Range<usize>, frame: &Range<usize>) -> bool {
        (self.retained_start, self.retained_end) == (retained.start, retained.end)
            && (self.trimmed_start, self.trimmed_end) == (frame.start, frame.end)
    }
}

/// Structural addresses remain attached to their own completion's parts.
#[derive(Debug)]
pub(crate) struct PartEnvelope {
    pub(crate) part: usize,
    pub(crate) reading: usize,
    pub(crate) source: SourceEnvelope,
}

impl ScratchCharge for PartEnvelope {
    const CHARGE: usize = 48;
}

/// Final outgoing parts and their sparse source certificates. The owner moves
/// through queued and held events without cloning its scratch reservation.
#[derive(Debug)]
pub(crate) struct CompletedParts {
    pub(crate) parts: Parts,
    pub(crate) envelopes: ScratchVec<PartEnvelope>,
}

impl ScratchCharge for CompletedParts {
    const CHARGE: usize = 64;
}

const _: () = {
    assert!(core::mem::size_of::<SourceEnvelope>() <= SourceEnvelope::CHARGE);
    assert!(core::mem::size_of::<PartEnvelope>() <= PartEnvelope::CHARGE);
    assert!(core::mem::size_of::<CompletedParts>() <= CompletedParts::CHARGE);
};

/// Certify only the projected dialect whose embedded framing needs this
/// evidence. All reads occur while the producing attempt retains its source.
pub(super) fn certify(
    parts: &[(Part, u64)],
    base: usize,
    source: &[u8],
    governor: &mut Governor,
) -> Result<ScratchVec<PartEnvelope>, StopReason> {
    let mut certificates = ScratchVec::new(governor);
    for (part, (candidate, _)) in parts.iter().enumerate() {
        governor.charge(Resource::Work, 1)?;
        let Part::Call(call) = candidate else {
            continue;
        };
        for (reading, raw) in call.readings.iter().enumerate() {
            governor.charge(Resource::Work, 1)?;
            if raw.dialect != DialectId::HermesStripped {
                continue;
            }
            let retained = retained_context(raw, governor)?;
            let bytes = retained
                .start
                .checked_sub(base)
                .zip(retained.end.checked_sub(base))
                .and_then(|(start, end)| source.get(start..end))
                .ok_or_else(|| {
                    StopReason::InvalidInput(
                        "completed call source retired before envelope certification".into(),
                    )
                })?;
            let (mut start, mut end) = (0, bytes.len());
            while start < end {
                governor.charge(Resource::Work, 1)?;
                if !matches!(bytes[start], b' ' | b'\t' | b'\r' | b'\n') {
                    break;
                }
                start += 1;
            }
            while start < end {
                governor.charge(Resource::Work, 1)?;
                if !matches!(bytes[end - 1], b' ' | b'\t' | b'\r' | b'\n') {
                    break;
                }
                end -= 1;
            }
            if start == 0 && end == bytes.len() {
                continue;
            }
            push_scratch(
                &mut certificates,
                PartEnvelope {
                    part,
                    reading,
                    source: SourceEnvelope {
                        retained_start: retained.start,
                        retained_end: retained.end,
                        trimmed_start: retained.start + start,
                        trimmed_end: retained.start + end,
                    },
                },
                governor,
            )?;
        }
    }
    Ok(certificates)
}

/// Only exterior extraction can move a context boundary. An enclosing native
/// protocol still protects every interior byte, including dropped fence text.
pub(crate) fn retained_context(
    reading: &RawCall,
    governor: &mut Governor,
) -> Result<Range<usize>, StopReason> {
    let mut span = reading.span.clone();
    for edit in &reading.edits {
        governor.charge(Resource::Work, 1)?;
        if edit.span.start == span.start
            && edit.span.end <= span.end
            && context_extraction(reading, edit, governor)?
        {
            span.start = edit.span.end;
        }
    }
    for edit in reading.edits.iter().rev() {
        governor.charge(Resource::Work, 1)?;
        if edit.span.end == span.end
            && edit.span.start >= span.start
            && context_extraction(reading, edit, governor)?
        {
            span.end = edit.span.start;
        }
    }
    Ok(span)
}

/// Retained capture bytes cannot be removed from Markdown protection.
fn context_extraction(
    reading: &RawCall,
    edit: &MarkupEdit,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    if edit.span.is_empty()
        || !matches!(
            edit.operator,
            EditOperator::ExtractProse | EditOperator::ExtractFence
        )
    {
        return Ok(false);
    }
    governor.charge(Resource::Work, size(reading.captures.len()))?;
    Ok(!reading.captures.iter().any(|capture| {
        capture.kind != CaptureKind::Dropped
            && capture.span.start < edit.span.end
            && edit.span.start < capture.span.end
    }))
}
