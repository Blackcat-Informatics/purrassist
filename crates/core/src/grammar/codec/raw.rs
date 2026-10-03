// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The `raw` codec: a verbatim body as one string.
use super::{
    Codec, CodecContext, CodecEnd, CodecFault, CodecKind, CodecValue, lossy, uncut, value,
};
use crate::contract::{Governor, StopReason};
use crate::json::{Node, Value};
use crate::tool_call::{ArgLeaf, ArgOrigin, ArgTree};
use core::ops::Range;

/// The `raw` codec: the verbatim body as one string.
///
/// A body cut off by a length stop in the middle of a multi-byte character
/// keeps its complete characters. Bytes that are not well-formed UTF-8 are
/// read as U+FFFD, one per maximal malformed subpart, each run recorded as
/// a `tool_call.value.invalid-utf8` edit (a data loss keeping the original
/// bytes and offsets), so the call is kept, at most a best guess. The codec
/// never faults and ignores its [`CodecContext`].
///
/// See the [module documentation](super) for an example.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawCodec {
    _body: (),
}

impl Codec for RawCodec {
    const KIND: CodecKind = CodecKind::Raw;

    fn begin(_at: usize, _governor: &mut Governor) -> Result<Self, StopReason> {
        Ok(Self { _body: () })
    }

    fn finish(
        self,
        body: &[u8],
        span: Range<usize>,
        end: CodecEnd,
        _context: CodecContext<'_>,
        governor: &mut Governor,
    ) -> Result<Result<CodecValue, CodecFault>, StopReason> {
        let keep = if end == CodecEnd::Truncated {
            uncut(body)
        } else {
            body.len()
        };
        let (text, edits) = lossy(&body[..keep], span.start, governor)?;
        let mut tree = ArgTree::value(Node {
            value: Value::String(text),
            span: Some(span.clone()),
        });
        tree.leaves.push(ArgLeaf {
            pointer: String::new(),
            origin: ArgOrigin::Raw,
            raw: span,
        });
        value(tree, edits, governor)
    }
}

#[cfg(test)]
mod tests {
    use super::{Codec, CodecContext, CodecEnd, CodecFault, RawCodec};
    use crate::grammar::codec::CodecStep;
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
    fn raw_reads_invalid_utf8_as_replacement() {
        assert_eq!(run("a€b".as_bytes(), CodecEnd::Delimited), Ok("a€b".into()));
        assert_eq!(run(b"a\xFFb", CodecEnd::Delimited), Ok("a\u{FFFD}b".into()));
        // A surrogate encoding: three maximal malformed subparts.
        assert_eq!(
            run(b"a\xED\xA0\x80", CodecEnd::Delimited),
            Ok("a\u{FFFD}\u{FFFD}\u{FFFD}".into())
        );
        let cut = &"a€".as_bytes()[..3];
        assert_eq!(run(cut, CodecEnd::Truncated), Ok("a".into()));
        assert_eq!(run(cut, CodecEnd::EndOfInput), Ok("a\u{FFFD}".into()));
        // One edit per run, exact span and original bytes.
        let mut governor = Governor::new(Budget::default());
        let body = b"x\xE9\xFFy\xC3";
        let value = RawCodec::begin(5, &mut governor)
            .unwrap()
            .finish(
                body,
                5..10,
                CodecEnd::Delimited,
                CodecContext::default(),
                &mut governor,
            )
            .unwrap()
            .unwrap();
        let edits: Vec<_> = value
            .edits
            .iter()
            .map(|edit| {
                (
                    edit.span.clone(),
                    edit.original.clone(),
                    edit.replacement.clone(),
                )
            })
            .collect();
        assert_eq!(
            edits,
            [
                (
                    6..8,
                    b"\xE9\xFF".to_vec(),
                    "\u{FFFD}\u{FFFD}".as_bytes().to_vec()
                ),
                (9..10, b"\xC3".to_vec(), "\u{FFFD}".as_bytes().to_vec()),
            ]
        );
    }
}
