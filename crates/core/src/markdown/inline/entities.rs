// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Semicolon-required Markdown references with bounded lookahead.
use super::entity_data::NAMED;
use std::borrow::Cow;

/// Decode exactly one reference at the start, never recursively interpreting output.
pub(super) fn one(source: &str) -> Option<(usize, Cow<'static, str>)> {
    let rest = source.strip_prefix('&')?;
    if let Some(numeric) = rest.strip_prefix('#') {
        let (digits, radix, limit, prefix) = numeric
            .strip_prefix(['x', 'X'])
            .map_or((numeric, 10, 7, 2), |hex| (hex, 16, 6, 3));
        let length = digits
            .bytes()
            .take(limit + 1)
            .take_while(|byte| {
                if radix == 16 {
                    byte.is_ascii_hexdigit()
                } else {
                    byte.is_ascii_digit()
                }
            })
            .count();
        if length == 0 || length > limit || digits.as_bytes().get(length) != Some(&b';') {
            return None;
        }
        let code = u32::from_str_radix(&digits[..length], radix).ok()?;
        let character = char::from_u32(code)
            .filter(|c| *c != '\0')
            .unwrap_or('\u{fffd}');
        return Some((prefix + length + 1, Cow::Owned(character.to_string())));
    }
    // The longest standardized name has 31 letters. No suffix-wide scan.
    let length = rest
        .bytes()
        .take(32)
        .take_while(u8::is_ascii_alphanumeric)
        .count();
    if length == 0 || length > 31 || rest.as_bytes().get(length) != Some(&b';') {
        return None;
    }
    let width = length + 2;
    let key = &source[..width];
    let index = NAMED.binary_search_by_key(&key, |(name, _)| *name).ok()?;
    Some((width, Cow::Borrowed(NAMED[index].1)))
}

/// Decode title/destination text after syntax recognition, protecting escaped ampersands.
pub(in crate::markdown) fn text(source: &str) -> String {
    let normalized = super::normalize_lines(source);
    let source = normalized.as_str();
    let mut output = String::new();
    let mut position = 0;
    while position < source.len() {
        let rest = &source[position..];
        if rest.starts_with('\\') && rest.as_bytes().get(1).is_some_and(u8::is_ascii_punctuation) {
            output.push(char::from(rest.as_bytes()[1]));
            position += 2;
        } else if let Some((width, value)) = one(rest) {
            output.push_str(&value);
            position += width;
        } else {
            let character = rest.chars().next().expect("remaining character");
            output.push(character);
            position += character.len_utf8();
        }
    }
    output
}
