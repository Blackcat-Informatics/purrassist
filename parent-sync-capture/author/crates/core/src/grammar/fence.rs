// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! The shared suffix authority for a complete protocol fence line.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LineSuffix {
    Horizontal,
    CarriageReturn,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LineDecision {
    More,
    End,
    Invalid,
}
impl LineSuffix {
    pub(crate) const fn step(&mut self, byte: u8) -> LineDecision {
        match (*self, byte) {
            (Self::Horizontal, b' ' | b'\t') => LineDecision::More,
            (Self::Horizontal, b'\r') => {
                *self = Self::CarriageReturn;
                LineDecision::More
            }
            (_, b'\n') => LineDecision::End,
            _ => LineDecision::Invalid,
        }
    }
    pub(crate) const fn at_end(self) -> bool {
        matches!(self, Self::Horizontal)
    }
}

/// Whether a suffix already proves a complete closing line.
/// The caller admits the bounded byte scan before calling this helper.
pub(crate) fn complete(bytes: &[u8], eof: bool) -> bool {
    let mut suffix = LineSuffix::Horizontal;
    for &byte in bytes {
        match suffix.step(byte) {
            LineDecision::More => {}
            LineDecision::End => return true,
            LineDecision::Invalid => return false,
        }
    }
    eof && suffix.at_end()
}
