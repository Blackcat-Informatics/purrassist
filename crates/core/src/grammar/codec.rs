// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Argument codecs: the sub-machines that read a codec slot's body.
//!
//! A slot `slot(CODEC, "delimiter", ...)` hands every body byte to its
//! codec, in order, once the engine knows the byte cannot begin one of the
//! slot's delimiters (the engine holds back at most the longest delimiter's
//! length). The codec either consumes the byte, reports that its value
//! ended before it ([`CodecStep::Complete`], for self-delimiting codecs), or
//! reports a [`CodecFault`]. When the body ends, at a delimiter or at the
//! end of input, [`Codec::finish`] turns it into a source-spanned
//! [`ArgTree`].
//!
//! Codecs form a closed set ([`CodecKind`]) dispatched by `match` over the
//! engine's per-slot codec state: there is no dynamic dispatch per byte.
//! Adding a codec means adding a [`CodecKind`] variant, its [`Codec`]
//! implementation, and its arm in each state dispatch; the engine itself is
//! unchanged.
//!
//! | Codec | Slot name | Value |
//! | --- | --- | --- |
//! | [`RawCodec`] | `raw` | the verbatim body as one string leaf |
//!
//! ```
//! use purrassist_core::grammar::codec::{Codec, CodecEnd, CodecStep, RawCodec};
//! use purrassist_core::{Budget, Governor};
//! let mut governor = Governor::new(Budget::default());
//! let body = "héllo".as_bytes();
//! let mut codec = RawCodec::begin(10);
//! for (index, &byte) in body.iter().enumerate() {
//!     assert_eq!(codec.feed(byte, 10 + index, &mut governor)?, CodecStep::Consumed);
//! }
//! let tree = codec.finish(body, 10..16, CodecEnd::Delimited, &mut governor)?.unwrap();
//! assert_eq!(tree.root.as_str(), Some("héllo"));
//! assert_eq!(tree.leaves[0].raw, 10..16);
//! # Ok::<(), purrassist_core::StopReason>(())
//! ```
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, ScratchCharge, StopReason};
use crate::json::{Node, Value};
use crate::tool_call::{ArgLeaf, ArgOrigin, ArgTree, LEAF_CHARGE};
use core::ops::Range;

/// The closed set of argument codecs a slot can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CodecKind {
    /// Verbatim capture: the body is one UTF-8 string.
    Raw,
}

impl CodecKind {
    /// Every codec, in declaration order.
    pub const ALL: [Self; 1] = [Self::Raw];

    /// The slot name this codec is registered under.
    ///
    /// ```
    /// use purrassist_core::grammar::codec::CodecKind;
    /// assert_eq!(CodecKind::Raw.name(), "raw");
    /// ```
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Raw => "raw",
        }
    }

    /// The codec registered under `name`.
    ///
    /// ```
    /// use purrassist_core::grammar::codec::CodecKind;
    /// assert_eq!(CodecKind::from_name("raw"), Some(CodecKind::Raw));
    /// assert_eq!(CodecKind::from_name("yaml"), None);
    /// ```
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "raw" => Some(Self::Raw),
            _ => None,
        }
    }

    pub(crate) fn begin(self, at: usize) -> CodecState {
        match self {
            Self::Raw => CodecState::Raw(RawCodec::begin(at)),
        }
    }
}

/// What a codec did with one body byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecStep {
    /// The byte belongs to the value.
    Consumed,
    /// The value ended before this byte, which is not consumed.
    Complete,
    /// The body cannot be decoded.
    Fault(CodecFault),
}

/// Why a codec cannot decode its body.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CodecFault {
    /// The body is not well-formed UTF-8; `at` is the absolute offset of
    /// the first byte of the malformed sequence.
    InvalidUtf8 {
        /// Absolute offset of the malformed sequence.
        at: usize,
    },
}

/// How a slot body ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecEnd {
    /// At one of the slot's delimiters.
    Delimited,
    /// At the end of input, under a stop that completes the output.
    EndOfInput,
    /// At the end of input, under a length stop: the body was cut off.
    Truncated,
}

/// One argument codec: a push-based sub-machine over a slot body.
pub trait Codec: Sized {
    /// The kind this implementation provides.
    const KIND: CodecKind;

    /// A codec for a body starting at absolute offset `at`.
    fn begin(at: usize) -> Self;

    /// Offer the next body byte, at absolute offset `at`.
    ///
    /// # Errors
    /// Returns the governor's exhaustion or cancellation reason.
    fn feed(
        &mut self,
        byte: u8,
        at: usize,
        governor: &mut Governor,
    ) -> Result<CodecStep, StopReason>;

    /// Build the value of the whole `body`, whose absolute range is `span`,
    /// charging exactly the value's [`ArgTree::estimated_bytes`] of
    /// `ScratchBytes` before allocating it.
    ///
    /// # Errors
    /// The outer `Err` is the governor's exhaustion or cancellation reason;
    /// `Ok(Err(fault))` reports a body the codec cannot decode.
    fn finish(
        self,
        body: &[u8],
        span: Range<usize>,
        end: CodecEnd,
        governor: &mut Governor,
    ) -> Result<Result<ArgTree, CodecFault>, StopReason>;
}

/// The `raw` codec: the verbatim body as one string, validated as UTF-8
/// incrementally (one comparison per byte).
///
/// A body cut off by a length stop in the middle of a multi-byte character
/// keeps its complete characters; any other malformed sequence is a
/// [`CodecFault::InvalidUtf8`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawCodec {
    remaining: u8,
    low: u8,
    high: u8,
    pending: u8,
    sequence: usize,
    fault: Option<usize>,
}

impl Codec for RawCodec {
    const KIND: CodecKind = CodecKind::Raw;

    fn begin(_at: usize) -> Self {
        Self {
            remaining: 0,
            low: 0x80,
            high: 0xBF,
            pending: 0,
            sequence: 0,
            fault: None,
        }
    }

    fn feed(
        &mut self,
        byte: u8,
        at: usize,
        _governor: &mut Governor,
    ) -> Result<CodecStep, StopReason> {
        if let Some(at) = self.fault {
            return Ok(CodecStep::Fault(CodecFault::InvalidUtf8 { at }));
        }
        if self.remaining == 0 {
            let (remaining, low, high) = match byte {
                0x00..=0x7F => return Ok(CodecStep::Consumed),
                0xC2..=0xDF => (1, 0x80, 0xBF),
                0xE0 => (2, 0xA0, 0xBF),
                0xED => (2, 0x80, 0x9F),
                0xE1..=0xEF => (2, 0x80, 0xBF),
                0xF0 => (3, 0x90, 0xBF),
                0xF1..=0xF3 => (3, 0x80, 0xBF),
                0xF4 => (3, 0x80, 0x8F),
                _ => return Ok(self.fail(at)),
            };
            self.remaining = remaining;
            self.low = low;
            self.high = high;
            self.pending = 1;
            self.sequence = at;
            return Ok(CodecStep::Consumed);
        }
        if !(self.low..=self.high).contains(&byte) {
            return Ok(self.fail(self.sequence));
        }
        self.remaining -= 1;
        self.pending += 1;
        self.low = 0x80;
        self.high = 0xBF;
        if self.remaining == 0 {
            self.pending = 0;
        }
        Ok(CodecStep::Consumed)
    }

    fn finish(
        self,
        body: &[u8],
        span: Range<usize>,
        end: CodecEnd,
        governor: &mut Governor,
    ) -> Result<Result<ArgTree, CodecFault>, StopReason> {
        if let Some(at) = self.fault {
            return Ok(Err(CodecFault::InvalidUtf8 { at }));
        }
        let keep = if self.remaining == 0 {
            body.len()
        } else if end == CodecEnd::Truncated {
            body.len() - usize::from(self.pending)
        } else {
            return Ok(Err(CodecFault::InvalidUtf8 { at: self.sequence }));
        };
        // Exactly the tree's `estimated_bytes`: one string node and one leaf.
        governor.charge(
            Resource::ScratchBytes,
            size(<Node as ScratchCharge>::CHARGE)
                .saturating_add(size(keep))
                .saturating_add(LEAF_CHARGE),
        )?;
        let Ok(text) = String::from_utf8(body[..keep].to_vec()) else {
            return Ok(Err(CodecFault::InvalidUtf8 { at: span.start }));
        };
        let raw = span.clone();
        Ok(Ok(ArgTree {
            root: Node {
                value: Value::String(text),
                span: Some(span),
            },
            leaves: vec![ArgLeaf {
                pointer: String::new(),
                origin: ArgOrigin::Raw,
                raw,
            }],
        }))
    }
}

impl RawCodec {
    const fn fail(&mut self, at: usize) -> CodecStep {
        self.fault = Some(at);
        CodecStep::Fault(CodecFault::InvalidUtf8 { at })
    }
}

/// The live state of one codec slot: one variant per [`CodecKind`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CodecState {
    /// A [`RawCodec`].
    Raw(RawCodec),
}

impl CodecState {
    pub(crate) fn feed(
        &mut self,
        byte: u8,
        at: usize,
        governor: &mut Governor,
    ) -> Result<CodecStep, StopReason> {
        match self {
            Self::Raw(codec) => codec.feed(byte, at, governor),
        }
    }

    pub(crate) fn finish(
        self,
        body: &[u8],
        span: Range<usize>,
        end: CodecEnd,
        governor: &mut Governor,
    ) -> Result<Result<ArgTree, CodecFault>, StopReason> {
        match self {
            Self::Raw(codec) => codec.finish(body, span, end, governor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Codec, CodecEnd, CodecFault, CodecStep, RawCodec};
    use crate::{Budget, Governor};

    fn run(body: &[u8], end: CodecEnd) -> Result<String, CodecFault> {
        let mut governor = Governor::new(Budget::default());
        let mut codec = RawCodec::begin(0);
        for (at, &byte) in body.iter().enumerate() {
            if let CodecStep::Fault(fault) = codec.feed(byte, at, &mut governor).unwrap() {
                return Err(fault);
            }
        }
        codec
            .finish(body, 0..body.len(), end, &mut governor)
            .unwrap()
            .map(|tree| tree.root.as_str().unwrap().to_owned())
    }

    #[test]
    fn raw_charges_exactly_the_tree_estimate() {
        let mut governor = Governor::new(Budget::default());
        let body = b"verbatim body";
        let codec = RawCodec::begin(4);
        let before = governor.usage().total_scratch_charged;
        let tree = codec
            .finish(body, 4..4 + body.len(), CodecEnd::Delimited, &mut governor)
            .unwrap()
            .unwrap();
        let charged = governor.usage().total_scratch_charged - before;
        assert_eq!(charged, tree.estimated_bytes());
    }

    #[test]
    fn raw_validates_utf8_incrementally() {
        assert_eq!(run("a€b".as_bytes(), CodecEnd::Delimited), Ok("a€b".into()));
        assert_eq!(
            run(b"a\xFFb", CodecEnd::Delimited),
            Err(CodecFault::InvalidUtf8 { at: 1 })
        );
        assert_eq!(
            run(b"a\xED\xA0\x80", CodecEnd::Delimited),
            Err(CodecFault::InvalidUtf8 { at: 1 })
        );
        let cut = &"a€".as_bytes()[..3];
        assert_eq!(run(cut, CodecEnd::Truncated), Ok("a".into()));
        assert_eq!(
            run(cut, CodecEnd::EndOfInput),
            Err(CodecFault::InvalidUtf8 { at: 1 })
        );
    }
}
