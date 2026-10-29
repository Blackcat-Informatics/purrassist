// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! RFC 5321/5322 `addr-spec` (`email`) and its RFC 6531 SMTPUTF8
//! internationalization (`idn-email`): `local-part "@" domain`, where
//! `domain` reuses the `hostname`/`idn-hostname` validator, and `local-part`
//! is a dot-atom or a quoted string.
use super::host::is_hostname;
use super::net::{is_ipv4, is_ipv6};
use crate::contract::{Governor, Resource, StopReason};
use std::iter::Peekable;
use std::str::Chars;

type Cursor<'a> = Peekable<Chars<'a>>;

/// RFC 5322 `atext`, extended by RFC 6531 to accept any non-ASCII scalar
/// value when `idn` (SMTPUTF8 does not itself restrict which Unicode
/// scalars may appear; UTS-46-style category filtering belongs to the
/// domain part, not the local part).
const fn is_atext(c: char, idn: bool) -> bool {
    if !c.is_ascii() {
        return idn;
    }
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '!' | '#'
                | '$'
                | '%'
                | '&'
                | '\''
                | '*'
                | '+'
                | '-'
                | '/'
                | '='
                | '?'
                | '^'
                | '_'
                | '`'
                | '{'
                | '|'
                | '}'
                | '~'
        )
}

fn is_dot_atom(text: &str, idn: bool) -> bool {
    !text.is_empty()
        && text
            .split('.')
            .all(|label| !label.is_empty() && label.chars().all(|c| is_atext(c, idn)))
}

const fn quoted_pair_content(c: char) -> bool {
    c.is_ascii() && (c.is_ascii_graphic() || c == ' ' || c == '\t')
}

/// Consume a `quoted-string` body (the opening `"` already consumed),
/// including its closing `"`; `idn` extends `qtext` the same way `atext` is
/// extended above.
fn quoted_string(chars: &mut Cursor<'_>, idn: bool) -> bool {
    loop {
        match chars.next() {
            None => return false,
            Some('"') => return true,
            Some('\\') => {
                if !chars.next().is_some_and(quoted_pair_content) {
                    return false;
                }
            }
            Some(c) if c == '"' || c == '\\' => return false,
            Some(c) if c.is_ascii() => {
                if !(c.is_ascii_graphic() || c == ' ' || c == '\t') {
                    return false;
                }
            }
            Some(_) => {
                if !idn {
                    return false;
                }
            }
        }
    }
}

fn is_local_part(text: &str, idn: bool) -> bool {
    let mut chars = text.chars().peekable();
    if chars.peek() == Some(&'"') {
        chars.next();
        quoted_string(&mut chars, idn) && chars.next().is_none()
    } else {
        is_dot_atom(text, idn)
    }
}

/// `"[" ("IPv6:" IPv6address / IPv4address) "]"` (RFC 5321 section 4.1.3),
/// case-insensitive on the `IPv6:` tag.
fn is_domain_literal(text: &str, governor: &mut Governor) -> Result<bool, StopReason> {
    let Some(inner) = text
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    else {
        return Ok(false);
    };
    governor.charge(Resource::Work, 5)?; // fixed ASCII tag comparison
    if let Some(address) = strip_ascii_case_prefix(inner, "ipv6:") {
        is_ipv6(address, governor)
    } else {
        is_ipv4(inner, governor)
    }
}

fn strip_ascii_case_prefix<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    (text.len() >= prefix.len()
        && text.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes()))
    .then(|| &text[prefix.len()..])
}

pub(super) fn is_email(text: &str, idn: bool, governor: &mut Governor) -> Result<bool, StopReason> {
    let result = email(text, idn, governor)?;
    governor.check()?;
    Ok(result)
}
fn email(text: &str, idn: bool, governor: &mut Governor) -> Result<bool, StopReason> {
    // The quoted prefix scan and any following delimiter search visit disjoint
    // input ranges; the unquoted case is one delimiter search.
    governor.charge(
        Resource::Work,
        u64::try_from(text.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1),
    )?;
    let Some(at) = find_unquoted_at(text) else {
        return Ok(false);
    };
    let (local, domain) = (&text[..at], &text[at + 1..]);
    // Dot splitting and scalar classification each visit at most local.len()
    // bytes. The quoted alternative and its initial peek fit the same bound.
    governor.charge(
        Resource::Work,
        u64::try_from(local.len())
            .unwrap_or(u64::MAX)
            .saturating_mul(2)
            .saturating_add(2),
    )?;
    if !is_local_part(local, idn) {
        return Ok(false);
    }
    if domain.starts_with('[') {
        return is_domain_literal(domain, governor);
    }
    is_hostname(domain, idn, governor)
}

/// The first `@` that is not inside a quoted local-part; a quoted local
/// part's `@` is never a valid delimiter (RFC 5322 quoted-string content).
fn find_unquoted_at(text: &str) -> Option<usize> {
    if !text.starts_with('"') {
        return text.find('@');
    }
    let bytes = text.as_bytes();
    let mut index = 1;
    let mut escaped = false;
    while index < bytes.len() {
        match bytes[index] {
            _ if escaped => escaped = false,
            b'\\' => escaped = true,
            b'"' => return text[index + 1..].find('@').map(|offset| index + 1 + offset),
            _ => {}
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::is_email;
    use crate::{Budget, Governor};
    fn check(text: &str, idn: bool) -> bool {
        let mut governor = Governor::new(Budget::default());
        let valid = is_email(text, idn, &mut governor).expect("within budget");
        assert_eq!(governor.scratch_meter().live(), 0);
        valid
    }
    #[test]
    fn plain_email_enforces_ascii_dot_atom_and_quoting() {
        assert!(check("joe.bloggs@example.com", false));
        assert!(check("\"joe bloggs\"@example.com", false));
        assert!(check("\"\"@iana.org", false));
        assert!(check("joe.bloggs@[127.0.0.1]", false));
        assert!(check("joe.bloggs@[IPv6:::1]", false));
        assert!(!check(".test@example.com", false));
        assert!(!check("te..st@example.com", false));
        assert!(!check("a\u{e9}@iana.org", false));
        assert!(!check("\"test@iana.org", false));
        assert!(!check("test\"@iana.org", false));
    }
    #[test]
    fn idn_email_accepts_unicode_local_parts_and_domains() {
        assert!(check("δοκιμή@example.com", true));
        assert!(check("user@café.com", true));
        assert!(check("\u{85}@example.com", true));
        assert!(!check("δοκιμή@-example.com", true));
        assert!(!check("δοκιμή@example..com", true));
        assert!(!check("user\u{ff20}example.com", true));
    }
}
