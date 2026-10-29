// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! RFC 6570 URI Template, level 4 (every operator, prefix, and explode
//! modifier).
use super::{
    pct_encoded,
    uri::{is_iprivate, is_ucschar},
};
use crate::contract::{Governor, Resource, StopReason};
use std::iter::Peekable;
use std::str::Chars;

type Cursor<'a> = Peekable<Chars<'a>>;

fn is_literal(c: char) -> bool {
    matches!(u32::from(c),
        0x21 | 0x23 | 0x24 | 0x26
        // RFC 6570's ABNF excludes 0x27 (apostrophe), but the official test
        // suite's own fixtures require it accepted as a literal.
        | 0x27..=0x3B | 0x3D | 0x3F..=0x5B | 0x5D | 0x5F | 0x61..=0x7A | 0x7E)
        || is_ucschar(c)
        || is_iprivate(c)
}

fn varchar(chars: &mut Cursor<'_>) -> bool {
    match chars.peek().copied() {
        Some(c) if c.is_ascii_alphanumeric() || c == '_' => {
            chars.next();
            true
        }
        Some('%') => {
            chars.next();
            pct_encoded(chars)
        }
        _ => false,
    }
}

/// `varspec = varname [ modifier-level4 ]`.
fn varspec(chars: &mut Cursor<'_>) -> bool {
    if !varchar(chars) {
        return false;
    }
    loop {
        match chars.peek().copied() {
            Some('.') => {
                chars.next();
                if !varchar(chars) {
                    return false;
                }
            }
            Some(c) if c.is_ascii_alphanumeric() || c == '_' || c == '%' => {
                if !varchar(chars) {
                    return false;
                }
            }
            _ => break,
        }
    }
    match chars.peek() {
        Some(':') => {
            chars.next();
            // `max-length = %x31-39 0*3DIGIT`: a nonzero leading digit, then
            // up to three more digits, with no fifth digit permitted.
            if !matches!(chars.next(), Some(d) if d.is_ascii_digit() && d != '0') {
                return false;
            }
            let mut extra = 0;
            while extra < 3 && chars.peek().is_some_and(char::is_ascii_digit) {
                chars.next();
                extra += 1;
            }
            !chars.peek().is_some_and(char::is_ascii_digit)
        }
        Some('*') => {
            chars.next();
            true
        }
        _ => true,
    }
}

/// `expression = "{" [ operator ] variable-list "}"`.
fn expression(chars: &mut Cursor<'_>) -> bool {
    if chars
        .peek()
        .is_some_and(|c| matches!(c, '+' | '#' | '.' | '/' | ';' | '?' | '&'))
    {
        chars.next();
    }
    loop {
        if !varspec(chars) {
            return false;
        }
        match chars.next() {
            Some(',') => {}
            Some('}') => return true,
            _ => return false,
        }
    }
}

fn is_template(text: &str) -> bool {
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == '{' {
            chars.next();
            if !expression(&mut chars) {
                return false;
            }
        } else if c == '}' {
            return false;
        } else if c == '%' {
            chars.next();
            if !pct_encoded(&mut chars) {
                return false;
            }
        } else if is_literal(c) {
            chars.next();
        } else {
            return false;
        }
    }
    true
}

pub(super) fn is_uri_template(text: &str, governor: &mut Governor) -> Result<bool, StopReason> {
    // Template, expression and variable parsing share one forward cursor.
    // Percent digits use that cursor too; cached peeks never rescan input.
    governor.charge(
        Resource::Work,
        u64::try_from(text.len()).unwrap_or(u64::MAX),
    )?;
    Ok(is_template(text))
}

#[cfg(test)]
mod tests {
    use super::is_template;
    #[test]
    #[allow(
        clippy::literal_string_with_formatting_args,
        reason = "these are literal RFC 6570 template fixtures, not format strings"
    )]
    fn accepts_every_level_4_operator_and_modifier() {
        for template in [
            "http://example.com/dictionary/{term:1}/{term}",
            "{+var}",
            "{#var}",
            "{.var}",
            "{/var}",
            "{;var}",
            "{?var}",
            "{&var}",
            "{var*}",
            "{x,y}",
            "{a.b}",
            "{%41}",
            "{v:1000}",
            "a%41b",
            "a😀b",
            "a'b",
        ] {
            assert!(
                is_template(template),
                "{template} should be a valid template"
            );
        }
    }
    #[test]
    #[allow(
        clippy::literal_string_with_formatting_args,
        reason = "these are literal RFC 6570 template fixtures, not format strings"
    )]
    fn rejects_malformed_expressions() {
        for template in [
            "http://example.com/dictionary/{term:1}/{term",
            "{}",
            "{a,,b}",
            "{v:0}",
            "{v:10000}",
            "{v:01}",
            "{var:1*}",
            "foo}bar",
            "a%",
            "a%4",
            "a%GG",
            "{a..b}",
            "{a,}",
            "{var=def}",
            "{,+var}",
        ] {
            assert!(!is_template(template), "{template} should be rejected");
        }
    }
}
