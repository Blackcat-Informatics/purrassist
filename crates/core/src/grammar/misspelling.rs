// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Misspelled closers (`tool_call.markup.closer-misspelled`).
//!
//! A closer token a model wrote with one slip (`</functio>`, `</paramete>`,
//! `<|tool_call_ed|>`) is read as the closer it misspells when the slip is
//! one edit inside the closer's delimiters and no other closer of the
//! grammar is that close. An edit is one byte inserted, deleted or
//! substituted, or two adjacent bytes swapped (optimal string alignment
//! distance one). The token keeps the closer's *shape*: its leading and
//! trailing runs of bytes that are not ASCII letters or digits (`</` and
//! `>`, `<|` and `|>`, `[` and `]`) are exactly the closer's, and every byte
//! between them is a word byte (an ASCII letter, digit, `_`, `-` or `.`, or
//! a byte of a non-ASCII character), so a closer's delimiters and the text
//! around a tag are never read into one. The closer's inner word must be at
//! least [`MIN_INNER`] bytes long, so short tokens (`}`, `]`, `</a>`) are
//! never respelled.
//!
//! This is distinct from a grammar's declared `confusable` spellings
//! (`tool_call.markup.terminal-confusable`), which a grammar states for
//! specific characters (an ASCII `|` for a fullwidth `｜`): a misspelling is
//! any single slip, admitted only when it identifies one closer.
// `unreachable_pub` and `redundant_pub_crate` disagree on the right
// visibility for items of a crate-private module
// (https://github.com/rust-lang/rust-clippy/issues/6188); these items are
// shared with the engine and the `xml-params` codec, so `pub(crate)`
// (matching `unreachable_pub`) wins, as in `engine`.
#![allow(clippy::redundant_pub_crate)]

/// The shortest inner word of a closer that may be respelled.
pub(crate) const MIN_INNER: usize = 3;

/// Whether `byte` may appear inside a closer's inner word.
pub(crate) const fn word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.') || byte >= 0x80
}

/// The lengths of `spelling`'s leading and trailing delimiter runs (bytes
/// that are not ASCII letters or digits).
pub(crate) fn delimiters(spelling: &[u8]) -> (usize, usize) {
    let lead = spelling
        .iter()
        .take_while(|byte| !byte.is_ascii_alphanumeric())
        .count();
    let trail = spelling[lead..]
        .iter()
        .rev()
        .take_while(|byte| !byte.is_ascii_alphanumeric())
        .count();
    (lead, trail)
}

/// Whether `a` and `b` are at most one edit apart: one byte inserted,
/// deleted or substituted, or two adjacent bytes swapped.
pub(crate) fn within_one(a: &[u8], b: &[u8]) -> bool {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    match long.len() - short.len() {
        0 => {
            let differ: Vec<usize> = (0..a.len()).filter(|&at| a[at] != b[at]).collect();
            match differ.as_slice() {
                [] | [_] => true,
                &[first, second] => {
                    second == first + 1 && a[first] == b[second] && a[second] == b[first]
                }
                _ => false,
            }
        }
        1 => {
            let prefix = short
                .iter()
                .zip(long)
                .take_while(|(left, right)| left == right)
                .count();
            short[prefix..] == long[prefix + 1..]
        }
        _ => false,
    }
}

/// Whether `token` misspells `spelling`: it differs from it by exactly one
/// edit inside its delimiters, keeping its shape (see the module
/// documentation).
pub(crate) fn misspells(token: &[u8], spelling: &[u8]) -> bool {
    let (lead, trail) = delimiters(spelling);
    if spelling.len() < lead + trail + MIN_INNER
        || token == spelling
        || token.len() < lead + trail
        || !token.starts_with(&spelling[..lead])
        || !token.ends_with(&spelling[spelling.len() - trail..])
    {
        return false;
    }
    let inner = &token[lead..token.len() - trail];
    !inner.is_empty()
        && inner.iter().all(|&byte| word(byte))
        && within_one(inner, &spelling[lead..spelling.len() - trail])
}

/// The one class of `classes` (each a list of spellings) that `token`
/// misspells, when exactly one class does and no spelling of any class is
/// `token` itself; otherwise `None` (an exact spelling, or a token close to
/// two closers, is not a misspelling to repair).
pub(crate) fn misspelled_class<'a>(
    token: &[u8],
    classes: impl IntoIterator<Item = &'a [Vec<u8>]>,
) -> Option<usize> {
    let mut found = None;
    for (index, spellings) in classes.into_iter().enumerate() {
        if spellings
            .iter()
            .any(|spelling| spelling.as_slice() == token)
        {
            return None;
        }
        if spellings.iter().any(|spelling| misspells(token, spelling)) {
            if found.is_some() {
                return None;
            }
            found = Some(index);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{misspelled_class, misspells, within_one};

    #[test]
    fn one_edit_of_any_kind() {
        assert!(within_one(b"function", b"functio"));
        assert!(within_one(b"function", b"functionn"));
        assert!(within_one(b"function", b"funxtion"));
        assert!(within_one(b"function", b"fucntion"));
        assert!(!within_one(b"function", b"fuctio"));
        assert!(!within_one(b"function", b"ufnctoin"));
    }

    #[test]
    fn misspellings_keep_the_closer_shape() {
        assert!(misspells(b"</functio>", b"</function>"));
        assert!(misspells(b"</paramete>", b"</parameter>"));
        assert!(misspells(b"<|tool_call_ed|>", b"<|tool_call_end|>"));
        assert!(!misspells(b"</function>", b"</function>"));
        assert!(!misspells(b"</function", b"</function>"));
        assert!(!misspells(b"<function>", b"</function>"));
        assert!(!misspells(b"</func tion>", b"</function>"));
        assert!(!misspells(b"}", b"}"));
        assert!(!misspells(b"</ab>", b"</a>"));
    }

    #[test]
    fn a_token_close_to_two_closers_is_not_respelled() {
        let classes: [Vec<Vec<u8>>; 2] = [vec![b"</abcd>".to_vec()], vec![b"</abce>".to_vec()]];
        let slices = || classes.iter().map(Vec::as_slice);
        assert_eq!(misspelled_class(b"</abcf>", slices()), None);
        assert_eq!(misspelled_class(b"</abd>", slices()), Some(0));
        assert_eq!(misspelled_class(b"</abcd>", slices()), None);
    }
}
