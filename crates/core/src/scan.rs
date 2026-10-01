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
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, ScratchCharge, ScratchVec, StopReason};

/// One maximal bracketed region, as absolute byte offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    /// Offset of the opening `{` or `[`.
    pub start: usize,
    /// Offset just past the closing bracket, or the end of input when unclosed.
    pub end: usize,
    /// Whether the region's depth returned to zero.
    pub closed: bool,
}
impl ScratchCharge for Region {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 24);
        24
    };
}

/// A boundary reported by [`Tracker::push`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A region opened at this offset.
    Open(usize),
    /// A region closed.
    Close(Region),
}

/// A resumable, string- and escape-aware bracket-region tracker.
///
/// ```
/// use purrassist_core::scan::{Event, Region, Tracker};
/// let text = br#"say {"a": "}"} and [1, [2]"#;
/// let mut tracker = Tracker::new(0);
/// let events: Vec<Event> = text.iter().filter_map(|&byte| tracker.push(byte)).collect();
/// assert_eq!(
///     events,
///     [
///         Event::Open(4),
///         Event::Close(Region { start: 4, end: 14, closed: true }),
///         Event::Open(19),
///     ]
/// );
/// // The string `"}"` did not close the object; `[1, [2]` is still open.
/// assert_eq!(tracker.finish(), Some(Region { start: 19, end: 26, closed: false }));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tracker {
    offset: usize,
    depth: usize,
    start: usize,
    in_string: bool,
    escaped: bool,
}

impl Tracker {
    /// A tracker whose next byte is at absolute offset `offset`, outside any
    /// region.
    ///
    /// ```
    /// use purrassist_core::scan::{Event, Tracker};
    /// let mut tracker = Tracker::new(10);
    /// assert_eq!(tracker.push(b'['), Some(Event::Open(10)));
    /// assert_eq!(tracker.offset(), 11);
    /// ```
    #[must_use]
    pub const fn new(offset: usize) -> Self {
        Self {
            offset,
            depth: 0,
            start: offset,
            in_string: false,
            escaped: false,
        }
    }

    /// Absolute offset of the next byte.
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Current bracket depth (zero outside regions).
    ///
    /// ```
    /// use purrassist_core::scan::Tracker;
    /// let mut tracker = Tracker::new(0);
    /// for &byte in b"{[\"]\"" {
    ///     tracker.push(byte);
    /// }
    /// assert_eq!(tracker.depth(), 2);
    /// ```
    #[must_use]
    pub const fn depth(&self) -> usize {
        self.depth
    }

    /// Read one byte, reporting a region boundary when it is one.
    pub const fn push(&mut self, byte: u8) -> Option<Event> {
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
    #[must_use]
    pub const fn finish(self) -> Option<Region> {
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

/// Every maximal region of `input`, in order, in one pass.
///
/// # Errors
/// Charges one `Work` unit per byte and admits the returned list's storage
/// against `ScratchBytes`; returns the governor's exhaustion or cancellation
/// reason when either is refused.
///
/// ```
/// use purrassist_core::{Budget, Governor, scan::{Region, regions}};
/// let mut governor = Governor::new(Budget::default());
/// let found = regions(b"a {b} c [d", &mut governor).unwrap();
/// assert_eq!(
///     found,
///     [
///         Region { start: 2, end: 5, closed: true },
///         Region { start: 8, end: 10, closed: false },
///     ]
/// );
/// assert_eq!(governor.usage().work, 10);
/// ```
pub fn regions(input: &[u8], governor: &mut Governor) -> Result<Vec<Region>, StopReason> {
    governor.charge(Resource::Work, size(input.len()))?;
    let mut tracker = Tracker::new(0);
    let mut found = ScratchVec::new(governor);
    for &byte in input {
        if let Some(Event::Close(region)) = tracker.push(byte) {
            found.push(region)?;
        }
    }
    if let Some(region) = tracker.finish() {
        found.push(region)?;
    }
    Ok(found.into_vec())
}
