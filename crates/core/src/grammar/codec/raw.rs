// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The `raw` codec: a verbatim body as one string.
use super::{
    Codec, CodecContext, CodecEnd, CodecFault, CodecKind, CodecStep, CodecValue, Utf8Check, value,
};
use crate::contract::{Governor, StopReason};
use crate::json::{Node, Value};
use crate::tool_call::{ArgLeaf, ArgOrigin, ArgTree};
use core::ops::Range;

/// The `raw` codec: the verbatim body as one string, validated as UTF-8
/// incrementally (one comparison per byte).
///
/// A body cut off by a length stop in the middle of a multi-byte character
/// keeps its complete characters; any other malformed sequence is a
/// [`CodecFault::InvalidUtf8`], which rejects the call attempt. It records
/// no edits and ignores its [`CodecContext`].
///
/// See the [module documentation](super) for an example.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawCodec {
    utf8: Utf8Check,
}

impl Codec for RawCodec {
    const KIND: CodecKind = CodecKind::Raw;

    fn begin(_at: usize, _governor: &mut Governor) -> Result<Self, StopReason> {
        Ok(Self {
            utf8: Utf8Check::new(),
        })
    }

    fn feed(
        &mut self,
        byte: u8,
        at: usize,
        _governor: &mut Governor,
    ) -> Result<CodecStep, StopReason> {
        Ok(self.utf8.step(byte, at).map_or(CodecStep::Consumed, |at| {
            CodecStep::Fault(CodecFault::InvalidUtf8 { at })
        }))
    }

    fn finish(
        self,
        body: &[u8],
        span: Range<usize>,
        end: CodecEnd,
        _context: CodecContext<'_>,
        governor: &mut Governor,
    ) -> Result<Result<CodecValue, CodecFault>, StopReason> {
        let keep = match self.utf8.complete(body.len(), end) {
            Ok(keep) => keep,
            Err(at) => return Ok(Err(CodecFault::InvalidUtf8 { at })),
        };
        let Ok(text) = core::str::from_utf8(&body[..keep]) else {
            return Ok(Err(CodecFault::InvalidUtf8 { at: span.start }));
        };
        let mut tree = ArgTree::value(Node {
            value: Value::String(text.to_owned()),
            span: Some(span.clone()),
        });
        tree.leaves.push(ArgLeaf {
            pointer: String::new(),
            origin: ArgOrigin::Raw,
            raw: span,
        });
        value(tree, Vec::new(), governor)
    }
}

#[cfg(test)]
mod tests {
    use super::{Codec, CodecContext, CodecEnd, CodecFault, CodecStep, RawCodec};
    use crate::{Budget, Governor};

    fn run(body: &[u8], end: CodecEnd) -> Result<String, CodecFault> {
        let mut governor = Governor::new(Budget::default());
        let mut codec = RawCodec::begin(0, &mut governor).unwrap();
        for (at, &byte) in body.iter().enumerate() {
            if let CodecStep::Fault(fault) = codec.feed(byte, at, &mut governor).unwrap() {
                return Err(fault);
            }
        }
        codec
            .finish(
                body,
                0..body.len(),
                end,
                CodecContext::default(),
                &mut governor,
            )
            .unwrap()
            .map(|value| value.tree.root.as_str().unwrap().to_owned())
    }

    #[test]
    fn raw_charges_exactly_the_tree_estimate() {
        let mut governor = Governor::new(Budget::default());
        let body = b"verbatim body";
        let codec = RawCodec::begin(4, &mut governor).unwrap();
        let before = governor.usage().total_scratch_charged;
        let value = codec
            .finish(
                body,
                4..4 + body.len(),
                CodecEnd::Delimited,
                CodecContext::default(),
                &mut governor,
            )
            .unwrap()
            .unwrap();
        let charged = governor.usage().total_scratch_charged - before;
        assert_eq!(charged, value.tree.estimated_bytes());
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
