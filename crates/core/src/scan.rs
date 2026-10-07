// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Linear, push-based location of bracketed regions in surrounding text.
//!
//! A [`Tracker`] reads bytes one at a time, in O(1) time and space per byte,
//! and reports each *maximal* region: a `{` or `[` met outside any region
//! opens one, and it extends to the byte that brings the bracket depth back
//! to zero. Inside a region, double-quoted strings are honoured, including
//! backslash escapes, so brackets and quotes inside strings never change the
//! depth. Outside regions, text is prose: quotes and stray closers there are
//! ignored, since apostrophes and quotation marks in prose are not JSON.
//!
//! Depth counts brackets of either kind, so a closer of the wrong kind
//! still closes one level; repairing the mismatch is the parser's job.
//! A region still open at the end of the input extends to the end and is
//! reported as unclosed. Bytes of multi-byte UTF-8 characters are never
//! ASCII delimiters, so byte-level scanning is exact for UTF-8 text and the
//! result does not depend on how the input is split into calls.
// `unreachable_pub` and `redundant_pub_crate` disagree on the right
// visibility for items of a crate-private module
// (https://github.com/rust-lang/rust-clippy/issues/6188); `pub(crate)`
// (matching `unreachable_pub`) wins, as in `grammar::engine`.
#![allow(clippy::redundant_pub_crate)]
use crate::contract::ScratchCharge;

/// One maximal bracketed region, as absolute byte offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Region {
    /// Offset of the opening `{` or `[`.
    pub(crate) start: usize,
    /// Offset just past the closing bracket, or the end of input when unclosed.
    pub(crate) end: usize,
    /// Whether the region's depth returned to zero.
    pub(crate) closed: bool,
}
impl ScratchCharge for Region {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 24);
        24
    };
}

/// A boundary reported by [`Tracker::push`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    /// A region opened at this offset.
    Open(usize),
    /// A region closed.
    Close(Region),
}

/// A resumable, string- and escape-aware bracket-region tracker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tracker {
    offset: usize,
    depth: usize,
    start: usize,
    in_string: bool,
    escaped: bool,
}

impl Tracker {
    /// A tracker whose next byte is at absolute offset `offset`, outside any
    /// region.
    #[must_use]
    pub(crate) const fn new(offset: usize) -> Self {
        Self {
            offset,
            depth: 0,
            start: offset,
            in_string: false,
            escaped: false,
        }
    }

    /// Absolute offset of the next byte.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn offset(&self) -> usize {
        self.offset
    }

    /// Current bracket depth (zero outside regions).
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn depth(&self) -> usize {
        self.depth
    }

    /// Read one byte, reporting a region boundary when it is one.
    pub(crate) const fn push(&mut self, byte: u8) -> Option<Event> {
        let at = self.offset;
        self.offset += 1;
        if self.depth == 0 {
            if matches!(byte, b'{' | b'[') {
                self.depth = 1;
                self.start = at;
                return Some(Event::Open(at));
            }
            return None;
        }
        if self.in_string {
            if self.escaped {
                self.escaped = false;
            } else if byte == b'\\' {
                self.escaped = true;
            } else if byte == b'"' {
                self.in_string = false;
            }
            return None;
        }
        match byte {
            b'"' => self.in_string = true,
            b'{' | b'[' => self.depth += 1,
            b'}' | b']' => {
                self.depth -= 1;
                if self.depth == 0 {
                    return Some(Event::Close(Region {
                        start: self.start,
                        end: at + 1,
                        closed: true,
                    }));
                }
            }
            _ => {}
        }
        None
    }

    /// End of input: the region still open, if any, extended to the end.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn finish(self) -> Option<Region> {
        if self.depth == 0 {
            None
        } else {
            Some(Region {
                start: self.start,
                end: self.offset,
                closed: false,
            })
        }
    }
}

/// The examples of this crate-private module's documentation, run as unit
/// tests; some only show that a signature type-checks, so their helper
/// functions are never called.
#[cfg(test)]
#[allow(dead_code)]
mod doc_examples {
    #[test]
    fn example_0() {
        use crate::scan::{Event, Region, Tracker};
        let text = br#"say {"a": "}"} and [1, [2]"#;
        let mut tracker = Tracker::new(0);
        let events: Vec<Event> = text.iter().filter_map(|&byte| tracker.push(byte)).collect();
        assert_eq!(
            events,
            [
                Event::Open(4),
                Event::Close(Region {
                    start: 4,
                    end: 14,
                    closed: true
                }),
                Event::Open(19),
            ]
        );
        // The string `"}"` did not close the object; `[1, [2]` is still open.
        assert_eq!(
            tracker.finish(),
            Some(Region {
                start: 19,
                end: 26,
                closed: false
            })
        );
    }

    #[test]
    fn example_1() {
        use crate::scan::{Event, Tracker};
        let mut tracker = Tracker::new(10);
        assert_eq!(tracker.push(b'['), Some(Event::Open(10)));
        assert_eq!(tracker.offset(), 11);
    }

    #[test]
    fn example_2() {
        use crate::scan::Tracker;
        let mut tracker = Tracker::new(0);
        for &byte in b"{[\"]\"" {
            tracker.push(byte);
        }
        assert_eq!(tracker.depth(), 2);
    }
}
