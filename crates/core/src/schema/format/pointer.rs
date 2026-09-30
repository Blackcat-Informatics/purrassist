// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! RFC 6901 `json-pointer` and the draft `relative-json-pointer` extension.
use crate::contract::{Governor, Resource, StopReason};

/// `""` or a sequence of `"/" reference-token`, where `~` inside a token
/// must be immediately followed by `0` or `1` (RFC 6901 sections 3-4).
fn is_json_pointer_str(text: &str) -> bool {
    let mut characters = text.chars().peekable();
    while characters.peek().is_some() {
        if characters.next() != Some('/') {
            return false;
        }
        while let Some(&character) = characters.peek() {
            if character == '/' {
                break;
            }
            characters.next();
            if character == '~' && !matches!(characters.next(), Some('0' | '1')) {
                return false;
            }
        }
    }
    true
}

pub(super) fn is_json_pointer(text: &str, governor: &mut Governor) -> Result<bool, StopReason> {
    governor.charge(
        Resource::Work,
        u64::try_from(text.len()).unwrap_or(u64::MAX),
    )?;
    Ok(is_json_pointer_str(text))
}

pub(super) fn is_relative_json_pointer(
    text: &str,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(
        Resource::Work,
        u64::try_from(text.len()).unwrap_or(u64::MAX),
    )?;
    let bytes = text.as_bytes();
    let mut cursor = 0;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == 0 || (cursor > 1 && bytes[0] == b'0') {
        return Ok(false);
    }
    let rest = &text[cursor..];
    Ok(rest.is_empty() || rest == "#" || is_json_pointer_str(rest))
}

#[cfg(test)]
mod tests {
    use super::{is_json_pointer, is_relative_json_pointer};
    use crate::{Budget, Governor};
    fn pointer(text: &str) -> bool {
        is_json_pointer(text, &mut Governor::new(Budget::default())).expect("within budget")
    }
    fn relative(text: &str) -> bool {
        is_relative_json_pointer(text, &mut Governor::new(Budget::default())).expect("budget")
    }
    #[test]
    fn json_pointer_enforces_escape_and_leading_slash_rules() {
        assert!(pointer(""));
        assert!(pointer("/foo/bar~0/baz~1/%a"));
        assert!(pointer("/foo//bar"));
        assert!(pointer("/foo/bar/"));
        assert!(!pointer("/foo/bar~"));
        assert!(!pointer("a"));
        assert!(!pointer("/~2"));
        assert!(!pointer("#"));
    }
    #[test]
    fn relative_pointer_requires_a_leading_non_negative_integer() {
        assert!(relative("1"));
        assert!(relative("0/foo/bar"));
        assert!(relative("0#"));
        assert!(relative("0//"));
        assert!(!relative("/foo/bar"));
        assert!(!relative("01/a"));
        assert!(!relative("0##"));
        assert!(!relative("-1/foo/bar"));
        assert!(!relative(""));
    }
}
