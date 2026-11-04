// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Keep codec boxes owned through native use, context lookup and finish.

use crate::contract::{Governor, ScratchCharge, ScratchLease, StopReason};
use crate::grammar::codec::{
    CodecContext, CodecEnd, CodecFault, CodecKind, CodecState, CodecStep, CodecValue,
};
use core::ops::Range;

/// Construction and finish have separate lifetimes but the same control bound.
pub(super) const METADATA_BYTES: u64 = 640;
const _: () = assert!(
    core::mem::size_of::<(
        TakenCodec,
        CodecState,
        Result<FinishedCodec, StopReason>,
        CodecContext<'static>,
        Range<usize>,
        CodecEnd,
    )>() as u64
        <= METADATA_BYTES
);
const _: () = assert!(
    core::mem::size_of::<(
        OwnedCodec,
        CodecState,
        ScratchLease,
        ScratchLease,
        CodecKind,
        usize,
        u32,
        Result<CodecState, StopReason>,
    )>() as u64
        <= METADATA_BYTES
);
const _: () =
    assert!(core::mem::size_of::<(TakenCodec, ScratchLease, u64, u32)>() as u64 <= METADATA_BYTES);

const _: () = assert!(
    core::mem::size_of::<(FinishedCodec, CodecValue, Result<(u32, u64), StopReason>)>() as u64
        <= METADATA_BYTES
);

/// The box has one owner from allocation through destruction. Slot capacity
/// and every parser/tree/edit allocation retain their independent admissions.
#[derive(Debug)]
pub(super) struct OwnedCodec {
    // Fields drop in declaration order: physical state before its box lease.
    state: CodecState,
    boxed: ScratchLease,
}

impl ScratchCharge for Option<OwnedCodec> {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 96);
        96
    };
}

impl OwnedCodec {
    pub(super) const fn new(state: CodecState, boxed: ScratchLease) -> Self {
        Self { state, boxed }
    }

    pub(super) const fn kind(&self) -> CodecKind {
        self.state.kind()
    }

    pub(super) const fn boxed_bytes(&self) -> u64 {
        self.boxed.bytes()
    }

    pub(super) const fn in_string(&self) -> bool {
        self.state.in_string()
    }

    pub(super) const fn argument_anchor(&self) -> bool {
        self.state.argument_anchor()
    }

    pub(super) const fn complete_end(&self) -> Option<usize> {
        self.state.complete_end()
    }

    pub(super) fn observe_native_anchor(
        &mut self,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        self.state.observe_native_anchor(governor)
    }

    pub(super) fn feed(
        &mut self,
        byte: u8,
        at: usize,
        governor: &mut Governor,
    ) -> Result<CodecStep, StopReason> {
        self.state.feed(byte, at, governor)
    }

    fn finish(
        self,
        body: &[u8],
        span: Range<usize>,
        end: CodecEnd,
        context: CodecContext<'_>,
        governor: &mut Governor,
    ) -> Result<Result<CodecValue, CodecFault>, StopReason> {
        // Keep the original box lease until the complete native call returns,
        // including resource refusal and unwinding; no state can escape it.
        let Self { state, boxed } = self;
        let result = state.finish(body, span, end, context, governor);
        drop(boxed);
        result
    }
}

/// Keep the moved state and result carriers admitted during finalization.
pub(super) struct TakenCodec {
    codec: OwnedCodec,
    _metadata: ScratchLease,
}

/// Keep result controls admitted until the caller completes their disposition.
pub(super) struct FinishedCodec {
    pub(super) result: Result<CodecValue, CodecFault>,
    _metadata: ScratchLease,
}

impl TakenCodec {
    pub(super) const fn new(codec: OwnedCodec, metadata: ScratchLease) -> Self {
        Self {
            codec,
            _metadata: metadata,
        }
    }

    pub(super) const fn kind(&self) -> CodecKind {
        self.codec.kind()
    }

    pub(super) fn finish(
        self,
        body: &[u8],
        span: Range<usize>,
        end: CodecEnd,
        context: CodecContext<'_>,
        governor: &mut Governor,
    ) -> Result<FinishedCodec, StopReason> {
        let Self {
            codec,
            _metadata: metadata,
        } = self;
        let result = codec.finish(body, span, end, context, governor)?;
        Ok(FinishedCodec {
            result,
            _metadata: metadata,
        })
    }
}
